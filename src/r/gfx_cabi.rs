extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;

fn runtime_context_key() -> u32 {
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return 0x8000_0000 | vm_id as u32;
    }
    crate::percpu::this_cpu().cpu_index()
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
    pub enum FsNodeKind {
        File,
        Directory,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct FsStat {
        pub kind: FsNodeKind,
        pub len: u64,
    }

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
        crate::r::fs::trueosfs::primary_root_handle().ok_or(FsError::NoRoot)
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
            match crate::r::fs::trueosfs::file_out_async(disk, name.as_str()).await? {
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
            match crate::r::fs::trueosfs::file_info_async(disk, name.as_str()).await? {
                Some(info) => Ok(info.data_len as usize),
                None => Err(FsError::NotFound),
            }
        })
    }

    #[inline]
    pub fn stat(path: &str) -> Result<FsStat> {
        let disk = root_disk()?;
        let name = normalize_rel(path, true)?;
        crate::wait::spawn_and_wait_local(async move {
            if name.is_empty() {
                return Ok(FsStat {
                    kind: FsNodeKind::Directory,
                    len: 0,
                });
            }

            if let Some(info) = crate::r::fs::trueosfs::file_info_async(disk, name.as_str()).await?
            {
                return Ok(FsStat {
                    kind: FsNodeKind::File,
                    len: info.data_len,
                });
            }

            let marker = alloc::format!("{}/.keep", name);
            if crate::r::fs::trueosfs::file_exists_async(disk, marker.as_str()).await? {
                return Ok(FsStat {
                    kind: FsNodeKind::Directory,
                    len: 0,
                });
            }

            Err(FsError::NotFound)
        })
    }

    #[inline]
    pub fn write_file_begin(path: &str, total_len: u64) -> Result<u32> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::r::fs::trueosfs::file_write_begin_async(disk, name.as_str(), total_len)
                .await?
            {
                Some(h) => Ok(h),
                None => Err(FsError::NoSpace),
            }
        })
    }

    #[inline]
    pub fn create_dir_all(path: &str) -> Result<()> {
        let disk = root_disk()?;
        let name = normalize_rel(path, true)?;
        if name.is_empty() {
            return Ok(());
        }

        crate::wait::spawn_and_wait_local(async move {
            let mut prefix = String::new();
            for part in name.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);

                let marker = alloc::format!("{}/.keep", prefix);
                let ok = crate::r::fs::trueosfs::file_in_async(disk, marker.as_str(), &[]).await?;
                if !ok {
                    return Err(FsError::NoSpace);
                }
            }
            Ok(())
        })
    }

    #[inline]
    pub fn write_file_chunk(handle: u32, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        crate::wait::spawn_and_wait_local(async move {
            crate::r::fs::trueosfs::file_write_chunk_async(handle, data.as_slice()).await?;
            Ok(())
        })
    }

    #[inline]
    pub fn write_file_finish(handle: u32) -> Result<()> {
        crate::wait::spawn_and_wait_local(async move {
            crate::r::fs::trueosfs::file_write_finish_async(handle).await?;
            Ok(())
        })
    }

    #[inline]
    pub fn write_file_abort(handle: u32) -> Result<()> {
        crate::wait::spawn_and_wait_local(async move {
            crate::r::fs::trueosfs::file_write_abort_async(handle).await?;
            Ok(())
        })
    }

    #[inline]
    pub fn html_tree(max_entries: usize) -> Result<String> {
        let disk = root_disk()?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::r::fs::trueosfs::html_tree_async(disk, max_entries).await? {
                Some(v) => Ok(v),
                None => Err(FsError::NoRoot),
            }
        })
    }

    #[inline]
    pub fn json_all(max_entries: usize) -> Result<String> {
        let disk = root_disk()?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::r::fs::trueosfs::json_all_async(disk, max_entries).await? {
                Some(v) => Ok(v),
                None => Err(FsError::NoRoot),
            }
        })
    }

    #[inline]
    pub fn remove(path: &str) -> Result<()> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            let ok = crate::r::fs::trueosfs::file_delete_async(disk, name.as_str()).await?;
            if ok { Ok(()) } else { Err(FsError::NotFound) }
        })
    }

    #[inline]
    pub fn exists(path: &str) -> Result<bool> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            Ok(crate::r::fs::trueosfs::file_exists_async(disk, name.as_str()).await?)
        })
    }
}

pub mod env {
    use super::{BTreeMap, String, Vec};
    use crate::shell2::MatrixTarget;
    use core::{ffi::c_char, ptr, slice, str};

    const VM_CONTEXT_SLOTS: usize = crate::allcaps::hv::VM_ID_LIMIT;
    const HOST_CONTEXT_SLOTS: usize = 64;
    const CONTEXT_SLOTS: usize = VM_CONTEXT_SLOTS + HOST_CONTEXT_SLOTS;

    #[derive(Clone)]
    struct LaunchContext {
        args: Vec<String>,
        vars: BTreeMap<String, String>,
        console_target: Option<MatrixTarget>,
        app_fs_root: Option<String>,
    }

    static CONTEXTS: [spin::Mutex<Vec<LaunchContext>>; CONTEXT_SLOTS] =
        [const { spin::Mutex::new(Vec::new()) }; CONTEXT_SLOTS];

    unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> Option<&'a str> {
        if ptr.is_null() {
            return None;
        }

        let mut len = 0usize;
        while unsafe { *ptr.add(len) } != 0 {
            len = len.saturating_add(1);
        }

        str::from_utf8(unsafe { slice::from_raw_parts(ptr.cast::<u8>(), len) }).ok()
    }

    #[inline]
    fn context_slot() -> usize {
        if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
            return (vm_id as usize).min(VM_CONTEXT_SLOTS.saturating_sub(1));
        }
        VM_CONTEXT_SLOTS + (crate::percpu::this_cpu().cpu_index() as usize % HOST_CONTEXT_SLOTS)
    }

    fn context_stack() -> &'static spin::Mutex<Vec<LaunchContext>> {
        &CONTEXTS[context_slot()]
    }

    pub(crate) fn with_launch_context_console_and_fs_root<R>(
        args: Vec<String>,
        vars: BTreeMap<String, String>,
        console_target: Option<MatrixTarget>,
        app_fs_root: Option<String>,
        f: impl FnOnce() -> R,
    ) -> R {
        {
            let mut stack = context_stack().lock();
            stack.push(LaunchContext {
                args,
                vars,
                console_target,
                app_fs_root,
            });
        }

        let out = f();

        let mut stack = context_stack().lock();
        let _ = stack.pop();
        if stack.is_empty() {
            *stack = Vec::new();
        }

        out
    }

    pub fn arg_count() -> usize {
        let stack = context_stack().lock();
        stack.last().map(|ctx| ctx.args.len()).unwrap_or(0)
    }

    pub fn arg(index: usize) -> Option<String> {
        let stack = context_stack().lock();
        stack.last().and_then(|ctx| ctx.args.get(index)).cloned()
    }

    pub fn var(key: &str) -> Option<String> {
        let stack = context_stack().lock();
        stack.last().and_then(|ctx| ctx.vars.get(key)).cloned()
    }

    pub(crate) unsafe extern "C" fn getenv(name: *const c_char) -> *mut c_char {
        let Some(key) = (unsafe { cstr_to_str(name) }) else {
            return ptr::null_mut();
        };

        let Some(value) = var(key) else {
            return ptr::null_mut();
        };

        let mut bytes = Vec::with_capacity(value.len().saturating_add(1));
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);

        let ptr = bytes.as_mut_ptr();
        core::mem::forget(bytes);
        ptr.cast::<c_char>()
    }

    pub(crate) fn console_target() -> Option<MatrixTarget> {
        let stack = context_stack().lock();
        stack.last().and_then(|ctx| ctx.console_target.clone())
    }

    fn normalize_app_path(path: &str, allow_empty: bool) -> Option<String> {
        let mut out = String::new();
        let t = path.trim();
        if t.is_empty() {
            return allow_empty.then_some(out);
        }

        for part in t.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                return None;
            }
            if !out.is_empty() {
                out.push('/');
            }
            out.push_str(part);
        }

        if out.is_empty() && !allow_empty {
            return None;
        }
        Some(out)
    }

    pub(crate) fn resolve_fs_path(path: &str, allow_empty: bool) -> Option<String> {
        let stack = context_stack().lock();
        let app_fs_root = stack.last().and_then(|ctx| ctx.app_fs_root.clone());
        drop(stack);

        let Some(root) = app_fs_root else {
            return Some(String::from(path));
        };

        let rel = normalize_app_path(path, allow_empty)?;
        if rel.is_empty() {
            Some(root)
        } else if rel == "common" {
            Some(String::from("apps/common"))
        } else if let Some(shared_rel) = rel.strip_prefix("common/") {
            if shared_rel.is_empty() {
                Some(String::from("apps/common"))
            } else {
                Some(alloc::format!("apps/common/{}", shared_rel))
            }
        } else {
            Some(alloc::format!("{}/{}", root.trim_matches('/'), rel))
        }
    }
}

/// Console routing + C ABI entrypoints used by embedded C code (QuickJS etc).
pub mod cabi {
    include!("cabi_codes.rs");

    use super::{BTreeMap, String, VecDeque};
    use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

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

    fn process_vm_text_stream(stream: ConsoleStream, text: &str) {
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
            emit_console_stream_line(stream, line.as_str());
            emit_vm_console_stream_line(stream, line.as_str());
        }
    }

    fn process_text_stream(stream: ConsoleStream, text: &str) {
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
            emit_console_stream_line(stream, line.as_str());
            if let Some((source, level, message)) = parse_structured_guest_log(line.as_str()) {
                emit_guest_log_line(source, level, message);
            } else {
                emit_plain_stream_line(stream, line.as_str());
            }
        }
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
        let slice = core::slice::from_raw_parts(bytes, len);
        write_console_bytes(stream, slice);
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_uart1_shell_write(
        data_ptr: *const u8,
        data_len: usize,
    ) -> usize {
        if data_ptr.is_null() || data_len == 0 {
            return 0;
        }
        let data = core::slice::from_raw_parts(data_ptr, data_len);
        crate::shell2::uart1_com1::write_bytes(data);
        data_len
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_shell1_submit_input(
        data_ptr: *const u8,
        data_len: usize,
    ) -> usize {
        if data_ptr.is_null() || data_len == 0 {
            return 0;
        }
        let data = core::slice::from_raw_parts(data_ptr, data_len);
        crate::shell2::uart1_com1::inject_bytes(data)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_shell_attached_write(
        data_ptr: *const u8,
        data_len: usize,
    ) -> usize {
        if data_ptr.is_null() || data_len == 0 {
            return 0;
        }
        let data = core::slice::from_raw_parts(data_ptr, data_len);
        if let Some(target) = super::env::console_target() {
            return crate::shell2::raw_write_matrix_target(&target, data);
        }
        crate::shell2::uart1_com1::write_bytes(data);
        data_len
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_shell_attached_read_byte() -> i32 {
        if let Some(target) = super::env::console_target() {
            return crate::shell2::read_matrix_target_byte(&target)
                .map(i32::from)
                .unwrap_or(-1);
        }
        crate::shell2::uart1_com1::read_byte()
            .map(i32::from)
            .unwrap_or(-1)
    }

    fn copy_cabi_text(bytes: &[u8], out_ptr: *mut u8, out_cap: usize) -> isize {
        if out_ptr.is_null() || out_cap == 0 {
            return bytes.len() as isize;
        }
        if out_cap < bytes.len() {
            return bytes.len() as isize;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, bytes.len());
        }
        bytes.len() as isize
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
        crate::r::io::env::arg_count()
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
        let Some(arg) = crate::r::io::env::arg(index) else {
            return -1;
        };
        copy_cabi_text(arg.as_bytes(), out_ptr, out_cap)
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
        let key_bytes = core::slice::from_raw_parts(key_ptr, key_len);
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
        let Some(value) = crate::r::io::env::var(key) else {
            return -1;
        };
        copy_cabi_text(value.as_bytes(), out_ptr, out_cap)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_shell_command_registry_json(
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize {
        let json = crate::shell2::command_registry_json();
        copy_cabi_text(json.as_bytes(), out_ptr, out_cap)
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_shell_history_lines_all() -> usize {
        crate::shell2::history_total_lines()
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_shell_history_lines(
        start_line: usize,
        max_lines: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize {
        let text = crate::shell2::history_lines_text(start_line, max_lines);
        copy_cabi_text(text.as_bytes(), out_ptr, out_cap)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_shell2_print_line(
        data_ptr: *const u8,
        data_len: usize,
    ) -> usize {
        if data_ptr.is_null() || data_len == 0 {
            return 0;
        }
        let data = core::slice::from_raw_parts(data_ptr, data_len);
        let Ok(text) = core::str::from_utf8(data) else {
            return 0;
        };
        if let Some(target) = super::env::console_target() {
            crate::shell2::print_matrix_target_line(&target, text);
            if crate::hv::current_hull_guest_context_vm_id().is_some() {
                crate::hv::log_active_blueprint_console_line(format_args!("guest: {}", text));
            }
        } else {
            crate::shell2::print_shell_line(&crate::shell2::UART1_COM1_BACKEND, text);
        }
        data_len
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
            crate::hv::vmcall::guest_sleep_ms(ms);
            return;
        }
        if ms == 0 {
            crate::wait::spin_step();
            return;
        }
        let _ = crate::wait::spin_until_timeout(ms, || false);
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_thread_current_id() -> usize {
        if crate::hv::current_hull_guest_context_vm_id().is_some() {
            let (status, id) =
                trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_THREAD_CURRENT_ID, 0, 0);
            return if status == trueos_vm::vmcall::STATUS_OK {
                id as usize
            } else {
                0
            };
        }
        if let Some(vtid) = crate::t::th::vthread::current_id() {
            return vtid as usize;
        }
        crate::percpu::current_slot().saturating_add(1)
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

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_ntp_current_unix_seconds() -> u64 {
        crate::r::net::ntp::current_unix_seconds().unwrap_or(0)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_ntp_kernel_date_day_month_year(
        out_ptr: *mut u8,
        out_len: usize,
    ) -> usize {
        let s = crate::r::net::ntp::kernel_date_day_month_year();
        let bytes = s.as_bytes();
        if !out_ptr.is_null() && out_len != 0 {
            let n = bytes.len().min(out_len);
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, n);
        }
        bytes.len()
    }

    #[derive(Clone, Copy)]
    struct AllocMeta {
        size: usize,
        align: usize,
    }

    const VM_CABI_ALLOC_HEADER_BYTES: usize = 16;
    const CABI_VM_ALLOC_BUCKET_SHIFT: usize = 24;
    const CABI_VM_ALLOC_BUCKET_INIT: u32 = u32::MAX;

    static CABI_ALLOC_TABLE: spin::Mutex<alloc::collections::BTreeMap<usize, AllocMeta>> =
        spin::Mutex::new(alloc::collections::BTreeMap::new());
    static CABI_VM_ALLOC_FREE_BUCKET_BY_VM: [AtomicU32; crate::allcaps::hv::VM_ID_LIMIT] =
        [const { AtomicU32::new(CABI_VM_ALLOC_BUCKET_INIT) }; crate::allcaps::hv::VM_ID_LIMIT];

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

    #[inline]
    fn cabi_vm_alloc_vm_id() -> Option<u8> {
        crate::hv::current_hull_guest_context_vm_id()
            .or_else(crate::t::kernel_task_domain::guest_owned_alloc_vm_id)
    }

    fn cabi_vm_heap_owner(ptr: *const u8) -> Option<u8> {
        if ptr.is_null() {
            return None;
        }

        let addr = ptr as usize;
        for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
            let Some(stats) = crate::allocators::hv_guest_heap_stats_if_configured(vm_id as u8)
            else {
                continue;
            };
            if stats.initialized && addr >= stats.heap_start && addr < stats.heap_end {
                return Some(vm_id as u8);
            }
        }
        None
    }

    #[inline]
    fn cabi_vm_alloc_active() -> bool {
        cabi_vm_alloc_vm_id().is_some()
    }

    #[inline]
    fn cabi_vm_layout_for(size: usize) -> Option<alloc::alloc::Layout> {
        let total = size.checked_add(VM_CABI_ALLOC_HEADER_BYTES)?;
        cabi_layout_for(total, cabi_malloc_align())
    }

    unsafe fn cabi_vm_alloc_inner(size: usize) -> *mut u8 {
        let Some(layout) = cabi_vm_layout_for(size) else {
            return core::ptr::null_mut();
        };
        let base = alloc::alloc::alloc(layout);
        if base.is_null() {
            return core::ptr::null_mut();
        }
        (base as *mut usize).write(size);
        (base as *mut usize).add(1).write(cabi_malloc_align());
        base.add(VM_CABI_ALLOC_HEADER_BYTES)
    }

    #[inline]
    unsafe fn cabi_return_address(depth: usize) -> usize {
        #[cfg(target_arch = "x86_64")]
        {
            let rbp: usize;
            core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack, preserves_flags));
            let mut frame = rbp as *const usize;
            let mut remaining = depth;
            while remaining != 0 {
                if frame.is_null()
                    || !(frame as usize).is_multiple_of(core::mem::align_of::<usize>())
                {
                    return 0;
                }
                frame = *frame as *const usize;
                remaining -= 1;
            }
            if frame.is_null() || !(frame as usize).is_multiple_of(core::mem::align_of::<usize>())
            {
                0
            } else {
                *frame.add(1)
            }
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = depth;
            0
        }
    }

    fn log_cabi_vm_alloc_watermark(vm_id: u8, size: usize, ptr: *mut u8) {
        let Some(bucket_slot) = CABI_VM_ALLOC_FREE_BUCKET_BY_VM.get(vm_id as usize) else {
            return;
        };
        let stats = crate::allocators::hv_guest_heap_stats(vm_id);
        let bucket = (stats.free_bytes >> CABI_VM_ALLOC_BUCKET_SHIFT) as u32;
        let previous = bucket_slot.swap(bucket, Ordering::AcqRel);
        let should_log = ptr.is_null()
            || size >= 1024 * 1024
            || previous == CABI_VM_ALLOC_BUCKET_INIT
            || bucket != previous;
        if !should_log {
            return;
        }
        crate::log!(
            "cabi-alloc: vm{} size={} ptr=0x{:X} free={} largest={} blocks={} bucket={} prev={} caller=0x{:016X} caller1=0x{:016X} caller2=0x{:016X}\n",
            vm_id,
            size,
            ptr as usize,
            stats.free_bytes,
            stats.largest_free_block,
            stats.free_blocks,
            bucket,
            previous,
            unsafe { cabi_return_address(3) },
            unsafe { cabi_return_address(4) },
            unsafe { cabi_return_address(5) },
        );
    }

    unsafe fn cabi_vm_alloc_for_vm(vm_id: u8, size: usize) -> *mut u8 {
        let ptr = if crate::hv::current_hull_guest_context_vm_id().is_some() {
            cabi_vm_alloc_inner(size)
        } else {
            crate::allocators::with_hv_guest_alloc_domain(vm_id, || cabi_vm_alloc_inner(size))
                .unwrap_or(core::ptr::null_mut())
        };
        log_cabi_vm_alloc_watermark(vm_id, size, ptr);
        ptr
    }

    unsafe fn cabi_vm_alloc(size: usize) -> *mut u8 {
        let Some(vm_id) = cabi_vm_alloc_vm_id() else {
            return core::ptr::null_mut();
        };
        cabi_vm_alloc_for_vm(vm_id, size)
    }

    unsafe fn cabi_vm_meta(ptr: *const u8) -> Option<(usize, usize, *mut u8)> {
        if ptr.is_null() {
            return None;
        }
        let base = (ptr as *mut u8).sub(VM_CABI_ALLOC_HEADER_BYTES);
        let size = (base as *const usize).read();
        let align = (base as *const usize).add(1).read();
        Some((size, align, base))
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_alloc(size: usize) -> *mut u8 {
        if cabi_vm_alloc_active() {
            return cabi_vm_alloc(size);
        }
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
        if cabi_vm_alloc_active() || cabi_vm_heap_owner(ptr).is_some() {
            let Some((size, align, base)) = cabi_vm_meta(ptr) else {
                return;
            };
            let Some(layout) =
                cabi_layout_for(size.saturating_add(VM_CABI_ALLOC_HEADER_BYTES), align)
            else {
                return;
            };
            alloc::alloc::dealloc(base, layout);
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

        if let Some(vm_id) = cabi_vm_alloc_vm_id().or_else(|| cabi_vm_heap_owner(ptr)) {
            let Some((old_size, _, _)) = cabi_vm_meta(ptr) else {
                return core::ptr::null_mut();
            };
            let new_ptr = cabi_vm_alloc_for_vm(vm_id, size);
            if new_ptr.is_null() {
                return core::ptr::null_mut();
            }
            core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(old_size, size));
            trueos_cabi_free(ptr);
            return new_ptr;
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
        if cabi_vm_alloc_active() || cabi_vm_heap_owner(ptr).is_some() {
            return cabi_vm_meta(ptr).map(|(size, _, _)| size).unwrap_or(0);
        }
        CABI_ALLOC_TABLE
            .lock()
            .get(&(ptr as usize))
            .map(|m| m.size)
            .unwrap_or(0)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_heap_stats(
        out: *mut v::vcabi::TrueosCabiHeapStats,
    ) -> i32 {
        if out.is_null() {
            return -1;
        }
        let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() else {
            return -2;
        };
        let stats = crate::allocators::hv_guest_heap_stats(vm_id);
        let source = match stats.source {
            crate::allocators::HeapSourceKind::Unconfigured => 0,
            crate::allocators::HeapSourceKind::Arena => 1,
        };
        unsafe {
            (*out).heap_start = stats.heap_start;
            (*out).heap_end = stats.heap_end;
            (*out).usable_total = stats.usable_total;
            (*out).free_bytes = stats.free_bytes;
            (*out).largest_free_block = stats.largest_free_block;
            (*out).free_blocks = stats.free_blocks;
            (*out).initialized = u32::from(stats.initialized);
            (*out).source = source;
        }
        0
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
            return FS_ERR_TOO_LARGE as isize;
        }
        let Some(path) = super::env::resolve_fs_path(path, false) else {
            return FS_ERR_BAD_PATH as isize;
        };
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE as isize;
        }
        match super::kfs::read_file_len(path.as_str()) {
            Ok(len) => len as isize,
            Err(e) => fs_error_to_code(e) as isize,
        }
    }

    pub(crate) fn fs_read_file_chunk_host(path: &str, offset: usize, out: &mut [u8]) -> isize {
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE as isize;
        }
        let Some(path) = super::env::resolve_fs_path(path, false) else {
            return FS_ERR_BAD_PATH as isize;
        };
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
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
            Err(e) => fs_error_to_code(e) as isize,
        }
    }

    pub(crate) fn fs_write_begin_host(path: &str, total_len: u64) -> i64 {
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE as i64;
        }
        let Some(path) = super::env::resolve_fs_path(path, false) else {
            return FS_ERR_BAD_PATH as i64;
        };
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE as i64;
        }
        match super::kfs::write_file_begin(path.as_str(), total_len) {
            Ok(h) => h as i64,
            Err(e) => fs_error_to_code(e) as i64,
        }
    }

    pub(crate) fn fs_write_chunk_host(handle: u32, data: &[u8]) -> i32 {
        match super::kfs::write_file_chunk(handle, data) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    pub(crate) fn fs_write_finish_host(handle: u32) -> i32 {
        match super::kfs::write_file_finish(handle) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    pub(crate) fn fs_write_abort_host(handle: u32) -> i32 {
        match super::kfs::write_file_abort(handle) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    pub(crate) fn fs_create_dir_all_host(path: &str) -> i32 {
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        let Some(path) = super::env::resolve_fs_path(path, true) else {
            return FS_ERR_BAD_PATH;
        };
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        match super::kfs::create_dir_all(path.as_str()) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    pub(crate) fn fs_exists_host(path: &str) -> i32 {
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        let Some(path) = super::env::resolve_fs_path(path, false) else {
            return FS_ERR_BAD_PATH;
        };
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        match super::kfs::exists(path.as_str()) {
            Ok(true) => 1,
            Ok(false) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    pub(crate) fn fs_stat_host(path: &str, out_kind: &mut u32, out_len: &mut u64) -> i32 {
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        let Some(path) = super::env::resolve_fs_path(path, true) else {
            return FS_ERR_BAD_PATH;
        };
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
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
            Err(e) => fs_error_to_code(e),
        }
    }

    pub(crate) fn fs_remove_host(path: &str) -> i32 {
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        let Some(path) = super::env::resolve_fs_path(path, false) else {
            return FS_ERR_BAD_PATH;
        };
        if path.len() > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        match super::kfs::remove(path.as_str()) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
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
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr.add(offset), got);
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
        if data.is_empty() {
            let mut out = [0u8; 1];
            let (status, rc) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_BP_FS_WRITE_CHUNK,
                handle as u64,
                0,
                &[],
                &mut out,
            );
            if status != trueos_vm::vmcall::STATUS_OK {
                return FS_ERR_BAD_PARAM;
            }
            return vmcall_signed_i32(rc);
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
        let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
        let Ok(path) = core::str::from_utf8(path_bytes) else {
            return FS_ERR_BAD_UTF8 as isize;
        };
        if crate::hv::current_hull_guest_context_vm_id().is_some() {
            return guest_fs_read_file(path_bytes, out_ptr, out_cap);
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
        fs_read_file_chunk_host(path, 0, core::slice::from_raw_parts_mut(out_ptr, len as usize))
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
        if crate::hv::current_hull_guest_context_vm_id().is_some() {
            return guest_fs_write_begin(path_bytes, total_len, out_handle);
        }
        let rc = fs_write_begin_host(path, total_len);
        if rc <= 0 {
            return rc as i32;
        }
        *out_handle = rc as u32;
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
        let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
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
            core::slice::from_raw_parts(data_ptr, data_len)
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
        let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
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
        if out_kind.is_null() || out_len.is_null() {
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
        if crate::hv::current_hull_guest_context_vm_id().is_some() {
            return guest_fs_stat(path_bytes, out_kind, out_len);
        }
        fs_stat_host(path, &mut *out_kind, &mut *out_len)
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
            Ok(html) => {
                let bytes = html.as_bytes();
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
    pub unsafe extern "C" fn trueos_cabi_trueosfs_json_all(
        max_entries: u32,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize {
        let limit = if max_entries == 0 {
            100usize
        } else {
            core::cmp::min(max_entries as usize, 100usize)
        };

        match super::kfs::json_all(limit) {
            Ok(json) => {
                let bytes = json.as_bytes();
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

    // --- GFX C-ABI ---
    // This is the stable bridge between the in-kernel JS "WebGL" shim and the renderer.
    // It intentionally targets the gfx abstraction (`trueos_gfx_core`) rather than a GPU driver.

    use alloc::vec::Vec;
    use embassy_time::Timer;
    use trueos_gfx_core::{
        BlendDesc, BlendFactor, BufferDesc, BufferId, BufferUsage, ColorFormat, Command,
        CommandBuffer, Extent2D, GfxContext, ImageDesc, ImageFormat, ImageId, ImageRegion,
        MemoryType, PipelineDesc, PipelineId, RGB_VERTEX_SIZE, RgbVertexF32 as RgbVtx, SamplerDesc,
        SamplerFilter, SamplerWrap, ScissorRect as GfxScissorRect, ShaderId, SwapchainDesc,
        TEX_VERTEX_SIZE, TexCoordFormat, TexVertexF32 as TexVtx, VertexLayout, Viewport,
        read_rgb_vertex_f32_bytes as read_rgb_vtx, read_tex_vertex_f32_bytes as read_tex_vtx,
    };

    const GFX_CABI_VBUF_RING_LEN: usize = 3;
    // Shared draw chunk budget used by cmd-stream draw capture paths.
    const MAX_CMDSTREAM_DRAW_BYTES: usize = 64 * 1024;
    // Conservative pre-submit guard to avoid submit_3d request overflow.
    const MAX_EST_SUBMIT_BYTES: usize = 512 * 1024;
    // Verbose per-frame begin/end tracing is useful when debugging gfx-cabi pacing,
    // but too noisy for normal runs.
    const TEX_PIPELINE_FS_MASK_TAG_RAW: u32 = 0x4D41_534B;
    const TEX_PIPELINE_FS_RGBA_TAG_RAW: u32 = 0x5247_4241;
    const TEX_PIPELINE_FS_PARTICLE_TAG_RAW: u32 = 0x5052_5443;
    const ASYNC_TEX_STATUS_UNKNOWN: i32 = 0;
    const ASYNC_TEX_STATUS_PENDING: i32 = 1;
    const ASYNC_TEX_STATUS_READY: i32 = 2;
    const GFX_CABI_VM_HOST_ONLY_RC: i32 = -90;
    const ASYNC_PNG_DECODE_TASK_POOL_SIZE: usize = 4;
    const ASYNC_JPEG_DECODE_TASK_POOL_SIZE: usize = 4;
    static ASYNC_TEX_STATUS: spin::Mutex<Vec<i32>> = spin::Mutex::new(Vec::new());
    static ASYNC_JPEG_REQS: spin::Mutex<VecDeque<AsyncJpegUploadReq>> =
        spin::Mutex::new(VecDeque::new());
    static ASYNC_SVG_REQS: spin::Mutex<VecDeque<AsyncSvgUploadReq>> =
        spin::Mutex::new(VecDeque::new());
    const TEXTURE_UPLOAD_RING_CAP: usize = 64;
    static TEXTURE_UPLOAD_REQS: spin::Mutex<TextureWorkRing> =
        spin::Mutex::new(TextureWorkRing::new());
    static VM_TEXTURE_UPLOADS: spin::Mutex<Vec<VmTextureUploadPending>> =
        spin::Mutex::new(Vec::new());
    static ASYNC_SVG_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
    static TEXTURE_UPLOAD_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
    static ASYNC_SVG_WORKER_STARTED: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
    const VM_TEXTURE_OWNER_CAP: usize = 256;
    const VM_TEXTURE_META_CAP: usize = 256;
    const VM_TEXTURE_GUEST_ID_LIMIT: u32 = 8_191;
    const VM_TEXTURE_HOST_BASE: u32 = 50_000;
    const VM_TEXTURE_HOST_STRIDE: u32 = 8_192;
    static VM_TEXTURE_OWNER_GENERATION: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(1);
    static VM_TEXTURE_META_GENERATION: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(1);
    static VM_TEXTURE_OWNERS: spin::Mutex<[VmTextureOwner; VM_TEXTURE_OWNER_CAP]> =
        spin::Mutex::new([VmTextureOwner::EMPTY; VM_TEXTURE_OWNER_CAP]);
    static VM_TEXTURE_META: spin::Mutex<[VmTextureMeta; VM_TEXTURE_META_CAP]> =
        spin::Mutex::new([VmTextureMeta::EMPTY; VM_TEXTURE_META_CAP]);

    #[derive(Clone, Copy)]
    struct VmTextureOwner {
        ctx_key: u32,
        tex_id: u32,
        generation: u32,
        valid: bool,
    }

    impl VmTextureOwner {
        const EMPTY: Self = Self {
            ctx_key: 0,
            tex_id: 0,
            generation: 0,
            valid: false,
        };
    }

    #[derive(Clone, Copy)]
    struct VmTextureMeta {
        ctx_key: u32,
        tex_id: u32,
        width: u32,
        height: u32,
        generation: u32,
        valid: bool,
    }

    impl VmTextureMeta {
        const EMPTY: Self = Self {
            ctx_key: 0,
            tex_id: 0,
            width: 0,
            height: 0,
            generation: 0,
            valid: false,
        };
    }

    #[inline]
    fn gfx_cabi_vm_context() -> bool {
        crate::hv::current_guest_execution_context_vm_id().is_some()
    }

    struct AsyncJpegUploadReq {
        tex_id: u32,
        bytes: Vec<u8>,
    }

    struct AsyncSvgUploadReq {
        tex_id: u32,
        bytes: Vec<u8>,
    }

    struct TextureUploadReq {
        tex_id: u32,
        width: u32,
        height: u32,
        region: Option<ImageRegion>,
        rgba: Vec<u8>,
        sample_kind: TexSampleKind,
        repaint_window_id: u32,
        repaint_reason: &'static str,
        update_async_status: bool,
    }

    struct VmTextureUploadPending {
        vm_id: u8,
        guest_tex_id: u32,
        host_tex_id: u32,
        width: u32,
        height: u32,
        region: Option<ImageRegion>,
        sample_kind: TexSampleKind,
        rgba: Vec<u8>,
        received: usize,
    }

    struct TextureDrawRgbReq {
        tex_id: u32,
        clear_rgb: u32,
        verts: Vec<u8>,
        repaint_window_id: u32,
        repaint_reason: &'static str,
    }

    struct TextureDrawMandelbrotReq {
        tex_id: u32,
        ticks: u64,
        tick_hz: u64,
        repaint_window_id: u32,
        repaint_reason: &'static str,
    }

    struct TextureDrawTexReq {
        target_tex_id: u32,
        source_tex_id: u32,
        clear_rgb: u32,
        verts: Vec<u8>,
        particle_shader: bool,
        repaint_window_id: u32,
        repaint_reason: &'static str,
    }

    enum TextureWorkReq {
        Upload(TextureUploadReq),
        DrawRgb(TextureDrawRgbReq),
        DrawMandelbrot(TextureDrawMandelbrotReq),
        DrawTex(TextureDrawTexReq),
    }

    struct TextureWorkRing {
        slots: [Option<TextureWorkReq>; TEXTURE_UPLOAD_RING_CAP],
        head: usize,
        len: usize,
    }

    impl TextureWorkRing {
        const fn new() -> Self {
            Self {
                slots: [const { None }; TEXTURE_UPLOAD_RING_CAP],
                head: 0,
                len: 0,
            }
        }

        fn index(&self, offset: usize) -> usize {
            (self.head + offset) % TEXTURE_UPLOAD_RING_CAP
        }

        fn replace_matching<F>(
            &mut self,
            req: TextureWorkReq,
            mut matches: F,
        ) -> Result<(), TextureWorkReq>
        where
            F: FnMut(&TextureWorkReq) -> bool,
        {
            for offset in 0..self.len {
                let idx = self.index(offset);
                if let Some(existing) = self.slots[idx].as_ref() {
                    if matches(existing) {
                        self.slots[idx] = Some(req);
                        return Ok(());
                    }
                }
            }
            Err(req)
        }

        fn push_back(&mut self, req: TextureWorkReq) -> Result<(), TextureWorkReq> {
            if self.len == TEXTURE_UPLOAD_RING_CAP {
                return Err(req);
            }
            let idx = self.index(self.len);
            self.slots[idx] = Some(req);
            self.len += 1;
            Ok(())
        }

        fn pop_front(&mut self) -> Option<TextureWorkReq> {
            if self.len == 0 {
                return None;
            }
            let req = self.slots[self.head].take();
            self.head = self.index(1);
            self.len -= 1;
            req
        }

        fn push_front(&mut self, req: TextureWorkReq) -> Result<(), TextureWorkReq> {
            if self.len == TEXTURE_UPLOAD_RING_CAP {
                return Err(req);
            }
            self.head = (self.head + TEXTURE_UPLOAD_RING_CAP - 1) % TEXTURE_UPLOAD_RING_CAP;
            self.slots[self.head] = Some(req);
            self.len += 1;
            Ok(())
        }
    }

    const MAX_REASONABLE_TEX_ID: u32 = VM_TEXTURE_HOST_BASE
        + (crate::allcaps::hv::VM_ID_LIMIT as u32).saturating_mul(VM_TEXTURE_HOST_STRIDE);

    #[inline]
    fn vm_context_key_for_id(vm_id: u8) -> u32 {
        0x8000_0000 | vm_id as u32
    }

    #[inline]
    fn vm_guest_texture_id_valid(tex_id: u32, op: &'static str) -> bool {
        if tex_id == 0 {
            return false;
        }
        if tex_id > VM_TEXTURE_GUEST_ID_LIMIT {
            crate::log!(
                "gfx-cabi: reject vm texture id tex={} op={} max_guest={}\n",
                tex_id,
                op,
                VM_TEXTURE_GUEST_ID_LIMIT
            );
            return false;
        }
        true
    }

    pub fn host_texture_id_for_vm(vm_id: u8, tex_id: u32) -> u32 {
        if !vm_guest_texture_id_valid(tex_id, "host-texture-map") {
            return 0;
        }
        VM_TEXTURE_HOST_BASE
            .saturating_add((vm_id as u32).saturating_mul(VM_TEXTURE_HOST_STRIDE))
            .saturating_add(tex_id)
    }

    fn host_texture_id_for_current_context(tex_id: u32) -> u32 {
        match crate::hv::current_guest_execution_context_vm_id() {
            Some(vm_id) => host_texture_id_for_vm(vm_id, tex_id),
            None => tex_id,
        }
    }

    #[inline]
    fn reject_unreasonable_tex_id(tex_id: u32, op: &'static str) -> bool {
        if tex_id == 0 || tex_id <= MAX_REASONABLE_TEX_ID {
            return false;
        }
        crate::log!(
            "gfx-cabi: reject unreasonable tex_id={} op={} max={}\n",
            tex_id,
            op,
            MAX_REASONABLE_TEX_ID
        );
        true
    }

    #[inline]
    fn reject_unreasonable_tex_pair(
        target_tex_id: u32,
        source_tex_id: u32,
        op: &'static str,
    ) -> bool {
        let mut rejected = false;
        if target_tex_id > MAX_REASONABLE_TEX_ID {
            crate::log!(
                "gfx-cabi: reject unreasonable target_tex_id={} op={} max={}\n",
                target_tex_id,
                op,
                MAX_REASONABLE_TEX_ID
            );
            rejected = true;
        }
        if source_tex_id > MAX_REASONABLE_TEX_ID {
            crate::log!(
                "gfx-cabi: reject unreasonable source_tex_id={} op={} max={}\n",
                source_tex_id,
                op,
                MAX_REASONABLE_TEX_ID
            );
            rejected = true;
        }
        rejected
    }

    fn set_async_tex_status(tex_id: u32, status: i32) {
        if tex_id == 0 {
            return;
        }
        let idx = tex_id.saturating_sub(1) as usize;
        let mut statuses = ASYNC_TEX_STATUS.lock();
        if idx >= statuses.len() {
            statuses.resize(idx + 1, ASYNC_TEX_STATUS_UNKNOWN);
        }
        statuses[idx] = status;
    }

    fn get_async_tex_status(tex_id: u32) -> i32 {
        if tex_id == 0 {
            return ASYNC_TEX_STATUS_UNKNOWN;
        }
        ASYNC_TEX_STATUS
            .lock()
            .get(tex_id.saturating_sub(1) as usize)
            .copied()
            .unwrap_or(ASYNC_TEX_STATUS_UNKNOWN)
    }

    fn vm_texture_owned_by_ctx(ctx_key: u32, tex_id: u32) -> bool {
        if tex_id == 0 {
            return false;
        }
        VM_TEXTURE_OWNERS
            .lock()
            .iter()
            .any(|entry| entry.valid && entry.ctx_key == ctx_key && entry.tex_id == tex_id)
    }

    fn claim_vm_texture_id_for_ctx(ctx_key: u32, tex_id: u32, reason: &'static str) -> bool {
        if !vm_guest_texture_id_valid(tex_id, reason) {
            return false;
        }
        let generation = VM_TEXTURE_OWNER_GENERATION
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        let mut owners = VM_TEXTURE_OWNERS.lock();
        let mut replace_idx = 0usize;
        let mut oldest_generation = u32::MAX;

        for (idx, entry) in owners.iter_mut().enumerate() {
            if entry.valid && entry.ctx_key == ctx_key && entry.tex_id == tex_id {
                entry.generation = generation;
                return true;
            }
            if !entry.valid {
                replace_idx = idx;
                break;
            }
            if entry.generation < oldest_generation {
                oldest_generation = entry.generation;
                replace_idx = idx;
            }
        }

        owners[replace_idx] = VmTextureOwner {
            ctx_key,
            tex_id,
            generation,
            valid: true,
        };
        true
    }

    pub fn claim_vm_texture_id_for_vm(vm_id: u8, tex_id: u32, reason: &'static str) -> bool {
        claim_vm_texture_id_for_ctx(vm_context_key_for_id(vm_id), tex_id, reason)
    }

    fn claim_current_vm_texture_id(tex_id: u32, reason: &'static str) -> bool {
        if !gfx_cabi_vm_context() {
            return true;
        }
        claim_vm_texture_id_for_ctx(super::runtime_context_key(), tex_id, reason)
    }

    fn require_current_vm_texture_owner(tex_id: u32, op: &'static str) -> bool {
        if !gfx_cabi_vm_context() {
            return true;
        }
        if !vm_guest_texture_id_valid(tex_id, op) {
            return false;
        }
        let ctx_key = super::runtime_context_key();
        if vm_texture_owned_by_ctx(ctx_key, tex_id) {
            return true;
        }
        crate::log!(
            "gfx-cabi: reject unowned vm texture tex={} op={} ctx=0x{:08X}\n",
            tex_id,
            op,
            ctx_key
        );
        false
    }

    fn record_vm_texture_dimensions(tex_id: u32, width: u32, height: u32) {
        if !gfx_cabi_vm_context() {
            return;
        }
        if tex_id == 0 || width == 0 || height == 0 {
            return;
        }
        if !claim_current_vm_texture_id(tex_id, "vm-texture-meta-owner") {
            return;
        }
        if reject_unreasonable_tex_id(tex_id, "vm-texture-meta") {
            return;
        }

        let ctx_key = super::runtime_context_key();
        let generation = VM_TEXTURE_META_GENERATION
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        let mut meta = VM_TEXTURE_META.lock();
        let mut replace_idx = 0usize;
        let mut oldest_generation = u32::MAX;

        for (idx, entry) in meta.iter_mut().enumerate() {
            if entry.valid && entry.ctx_key == ctx_key && entry.tex_id == tex_id {
                entry.width = width;
                entry.height = height;
                entry.generation = generation;
                return;
            }
            if !entry.valid {
                replace_idx = idx;
                break;
            }
            if entry.generation < oldest_generation {
                oldest_generation = entry.generation;
                replace_idx = idx;
            }
        }

        meta[replace_idx] = VmTextureMeta {
            ctx_key,
            tex_id,
            width,
            height,
            generation,
            valid: true,
        };
    }

    fn vm_texture_dimensions(tex_id: u32) -> Option<(u32, u32)> {
        if tex_id == 0 {
            return None;
        }
        if !require_current_vm_texture_owner(tex_id, "vm-texture-dimensions-owner") {
            return None;
        }
        if reject_unreasonable_tex_id(tex_id, "vm-texture-dimensions") {
            return None;
        }

        let ctx_key = super::runtime_context_key();
        VM_TEXTURE_META
            .lock()
            .iter()
            .find(|entry| entry.valid && entry.ctx_key == ctx_key && entry.tex_id == tex_id)
            .map(|entry| (entry.width, entry.height))
    }

    fn enqueue_async_jpeg_upload(tex_id: u32, bytes: Vec<u8>) {
        ASYNC_JPEG_REQS
            .lock()
            .push_back(AsyncJpegUploadReq { tex_id, bytes });
    }

    fn enqueue_async_svg_upload(tex_id: u32, bytes: Vec<u8>) {
        ASYNC_SVG_REQS
            .lock()
            .push_back(AsyncSvgUploadReq { tex_id, bytes });
        ASYNC_SVG_WAIT.notify_one();
    }

    fn notify_texture_work_available() {
        if crate::hv::current_hull_guest_context_vm_id().is_some() {
            TEXTURE_UPLOAD_WAIT.notify_guest_signal();
        } else {
            TEXTURE_UPLOAD_WAIT.notify_one();
        }
    }

    fn take_async_jpeg_upload() -> Option<AsyncJpegUploadReq> {
        ASYNC_JPEG_REQS.lock().pop_front()
    }

    fn requeue_async_jpeg_upload_front(req: AsyncJpegUploadReq) {
        ASYNC_JPEG_REQS.lock().push_front(req);
    }

    fn take_async_svg_upload() -> Option<AsyncSvgUploadReq> {
        ASYNC_SVG_REQS.lock().pop_front()
    }

    fn enqueue_texture_upload(req: TextureUploadReq) {
        let tex_id = req.tex_id;
        let mut queue = TEXTURE_UPLOAD_REQS.lock();
        let work = TextureWorkReq::Upload(req);
        let work = match queue.replace_matching(work, |entry| match entry {
            TextureWorkReq::Upload(existing) => existing.tex_id == tex_id,
            TextureWorkReq::DrawRgb(_) => false,
            TextureWorkReq::DrawMandelbrot(_) => false,
            TextureWorkReq::DrawTex(_) => false,
        }) {
            Ok(()) => {
                notify_texture_work_available();
                return;
            }
            Err(work) => work,
        };
        let _ = queue.push_back(work).map_err(drop_texture_work_overflow);
        notify_texture_work_available();
    }

    fn enqueue_texture_draw_rgb(req: TextureDrawRgbReq) {
        let tex_id = req.tex_id;
        let mut queue = TEXTURE_UPLOAD_REQS.lock();
        let work = TextureWorkReq::DrawRgb(req);
        let work = match queue.replace_matching(work, |entry| match entry {
            TextureWorkReq::Upload(_) => false,
            TextureWorkReq::DrawRgb(existing) => existing.tex_id == tex_id,
            TextureWorkReq::DrawMandelbrot(_) => false,
            TextureWorkReq::DrawTex(_) => false,
        }) {
            Ok(()) => {
                notify_texture_work_available();
                return;
            }
            Err(work) => work,
        };
        let _ = queue.push_back(work).map_err(drop_texture_work_overflow);
        notify_texture_work_available();
    }

    fn enqueue_texture_draw_mandelbrot(req: TextureDrawMandelbrotReq) {
        let tex_id = req.tex_id;
        let mut queue = TEXTURE_UPLOAD_REQS.lock();
        let work = TextureWorkReq::DrawMandelbrot(req);
        let work = match queue.replace_matching(work, |entry| match entry {
            TextureWorkReq::Upload(_) => false,
            TextureWorkReq::DrawRgb(_) => false,
            TextureWorkReq::DrawMandelbrot(existing) => existing.tex_id == tex_id,
            TextureWorkReq::DrawTex(_) => false,
        }) {
            Ok(()) => {
                notify_texture_work_available();
                return;
            }
            Err(work) => work,
        };
        let _ = queue.push_back(work).map_err(drop_texture_work_overflow);
        notify_texture_work_available();
    }

    fn enqueue_texture_draw_tex(req: TextureDrawTexReq) {
        let target_tex_id = req.target_tex_id;
        let mut queue = TEXTURE_UPLOAD_REQS.lock();
        let work = TextureWorkReq::DrawTex(req);
        let work = match queue.replace_matching(work, |entry| match entry {
            TextureWorkReq::Upload(_) => false,
            TextureWorkReq::DrawRgb(_) => false,
            TextureWorkReq::DrawMandelbrot(_) => false,
            TextureWorkReq::DrawTex(existing) => existing.target_tex_id == target_tex_id,
        }) {
            Ok(()) => {
                notify_texture_work_available();
                return;
            }
            Err(work) => work,
        };
        let _ = queue.push_back(work).map_err(drop_texture_work_overflow);
        notify_texture_work_available();
    }

    fn take_texture_upload() -> Option<TextureWorkReq> {
        TEXTURE_UPLOAD_REQS.lock().pop_front()
    }

    fn request_texture_work_present(window_id: u32, host_tex_id: u32, reason: &'static str) {
        if window_id == 0 {
            let _ = crate::hv::request_deferred_blueprint_app_windows_for_host_texture(
                host_tex_id,
                reason,
            );
            return;
        }
        let host_window_id = crate::hv::host_blueprint_app_window_id(window_id);
        let _ = crate::r::ui2::request_window_content_present(host_window_id, reason);
    }

    fn vm_texture_upload_get_u32(payload: &[u8], offset: usize) -> Option<u32> {
        payload
            .get(offset..offset + 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn vm_texture_upload_put_u32(payload: &mut [u8], offset: usize, value: u32) {
        payload[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn vm_texture_upload_expected_len(
        width: u32,
        height: u32,
        region: Option<ImageRegion>,
    ) -> Option<usize> {
        match region {
            Some(region) => {
                if region.width == 0
                    || region.height == 0
                    || region.x.saturating_add(region.width) > width
                    || region.y.saturating_add(region.height) > height
                {
                    return None;
                }
                (region.width as usize)
                    .checked_mul(region.height as usize)?
                    .checked_mul(4)
            }
            None => checked_reasonable_rgba_len(width, height),
        }
    }

    pub fn handle_vm_texture_upload_begin(vm_id: u8, payload: &[u8]) -> i32 {
        const HEADER_LEN: usize = 40;
        if payload.len() < HEADER_LEN {
            return -1;
        }
        let guest_tex_id = vm_texture_upload_get_u32(payload, 0).unwrap_or(0);
        let width = vm_texture_upload_get_u32(payload, 4).unwrap_or(0);
        let height = vm_texture_upload_get_u32(payload, 8).unwrap_or(0);
        let region_flag = vm_texture_upload_get_u32(payload, 12).unwrap_or(0);
        let region = if region_flag == 0 {
            None
        } else {
            Some(ImageRegion {
                x: vm_texture_upload_get_u32(payload, 16).unwrap_or(0),
                y: vm_texture_upload_get_u32(payload, 20).unwrap_or(0),
                width: vm_texture_upload_get_u32(payload, 24).unwrap_or(0),
                height: vm_texture_upload_get_u32(payload, 28).unwrap_or(0),
            })
        };
        let sample_kind = match vm_texture_upload_get_u32(payload, 32).unwrap_or(u32::MAX) {
            0 => TexSampleKind::Mask,
            1 => TexSampleKind::Rgba,
            _ => return -2,
        };
        let total_len = vm_texture_upload_get_u32(payload, 36).unwrap_or(0) as usize;
        if guest_tex_id == 0 || width == 0 || height == 0 {
            return -3;
        }
        if !claim_vm_texture_id_for_vm(vm_id, guest_tex_id, "vm-texture-upload-begin") {
            return -4;
        }
        let host_tex_id = host_texture_id_for_vm(vm_id, guest_tex_id);
        if host_tex_id == 0 || reject_unreasonable_tex_id(host_tex_id, "vm-texture-upload-host") {
            return -5;
        }
        let Some(expected) = vm_texture_upload_expected_len(width, height, region) else {
            return -6;
        };
        if total_len < expected {
            return -7;
        }

        let mut uploads = VM_TEXTURE_UPLOADS.lock();
        uploads.retain(|upload| upload.vm_id != vm_id);
        let mut rgba = Vec::new();
        if rgba.try_reserve_exact(expected).is_err() {
            return -8;
        }
        rgba.resize(expected, 0);
        uploads.push(VmTextureUploadPending {
            vm_id,
            guest_tex_id,
            host_tex_id,
            width,
            height,
            region,
            sample_kind,
            rgba,
            received: 0,
        });
        0
    }

    pub fn handle_vm_texture_upload_chunk(vm_id: u8, offset: usize, payload: &[u8]) -> i32 {
        let mut uploads = VM_TEXTURE_UPLOADS.lock();
        let Some(upload) = uploads.iter_mut().find(|upload| upload.vm_id == vm_id) else {
            return -1;
        };
        if offset != upload.received {
            return -2;
        }
        let end = offset.saturating_add(payload.len());
        if end > upload.rgba.len() {
            return -3;
        }
        upload.rgba[offset..end].copy_from_slice(payload);
        upload.received = end;
        0
    }

    pub fn handle_vm_texture_upload_finish(vm_id: u8) -> i32 {
        let pending = {
            let mut uploads = VM_TEXTURE_UPLOADS.lock();
            let Some(idx) = uploads.iter().position(|upload| upload.vm_id == vm_id) else {
                return -1;
            };
            uploads.swap_remove(idx)
        };
        if pending.received != pending.rgba.len() {
            return -2;
        }
        record_vm_texture_dimensions(pending.guest_tex_id, pending.width, pending.height);
        enqueue_texture_upload(TextureUploadReq {
            tex_id: pending.host_tex_id,
            width: pending.width,
            height: pending.height,
            region: pending.region,
            rgba: pending.rgba,
            sample_kind: pending.sample_kind,
            repaint_window_id: 0,
            repaint_reason: "vm-upload-rgba",
            update_async_status: false,
        });
        0
    }

    fn vmcall_texture_rgba_upload_from_ptr(
        tex_id: u32,
        width: u32,
        height: u32,
        region: Option<ImageRegion>,
        data_ptr: *const u8,
        data_len: usize,
        sample_kind: TexSampleKind,
    ) -> i32 {
        if tex_id == 0 || width == 0 || height == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        let Some(expected) = vm_texture_upload_expected_len(width, height, region) else {
            return -3;
        };
        if data_len < expected || expected > u32::MAX as usize {
            return -4;
        }

        let mut header = [0u8; 40];
        vm_texture_upload_put_u32(&mut header, 0, tex_id);
        vm_texture_upload_put_u32(&mut header, 4, width);
        vm_texture_upload_put_u32(&mut header, 8, height);
        vm_texture_upload_put_u32(&mut header, 12, u32::from(region.is_some()));
        if let Some(region) = region {
            vm_texture_upload_put_u32(&mut header, 16, region.x);
            vm_texture_upload_put_u32(&mut header, 20, region.y);
            vm_texture_upload_put_u32(&mut header, 24, region.width);
            vm_texture_upload_put_u32(&mut header, 28, region.height);
        }
        vm_texture_upload_put_u32(
            &mut header,
            32,
            match sample_kind {
                TexSampleKind::Mask => 0,
                TexSampleKind::Rgba => 1,
            },
        );
        vm_texture_upload_put_u32(&mut header, 36, expected as u32);

        let mut out = [0u8; 0];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_GFX_TEXTURE_UPLOAD_BEGIN,
            0,
            0,
            &header,
            &mut out,
        );
        if status != trueos_vm::vmcall::STATUS_OK || (rc as i64 as i32) != 0 {
            return if status == trueos_vm::vmcall::STATUS_OK {
                rc as i64 as i32
            } else {
                -90
            };
        }

        let data = unsafe { core::slice::from_raw_parts(data_ptr, expected) };
        let mut offset = 0usize;
        while offset < expected {
            let end = core::cmp::min(offset + trueos_vm::vmcall::PAYLOAD_CAP, expected);
            let (status, rc) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_BP_GFX_TEXTURE_UPLOAD_CHUNK,
                offset as u64,
                0,
                &data[offset..end],
                &mut out,
            );
            if status != trueos_vm::vmcall::STATUS_OK || (rc as i64 as i32) != 0 {
                return if status == trueos_vm::vmcall::STATUS_OK {
                    rc as i64 as i32
                } else {
                    -91
                };
            }
            offset = end;
        }

        let (status, rc) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_GFX_TEXTURE_UPLOAD_FINISH, 0, 0);
        if status == trueos_vm::vmcall::STATUS_OK {
            rc as i64 as i32
        } else {
            -92
        }
    }

    fn log_texture_work_failed(
        op: &'static str,
        rc: i32,
        target_tex_id: u32,
        source_tex_id: u32,
        repaint_window_id: u32,
        reason: &'static str,
    ) {
        crate::log!(
            "gfx-cabi: texture work failed op={} rc={} target={} source={} repaint_window={} reason={}\n",
            op,
            rc,
            target_tex_id,
            source_tex_id,
            repaint_window_id,
            reason
        );
    }

    fn requeue_texture_work_front(req: TextureWorkReq) {
        let _ = TEXTURE_UPLOAD_REQS
            .lock()
            .push_front(req)
            .map_err(drop_texture_work_overflow);
        notify_texture_work_available();
    }

    fn drop_texture_work_overflow(req: TextureWorkReq) {
        let kind = match req {
            TextureWorkReq::Upload(_) => "upload",
            TextureWorkReq::DrawRgb(_) => "draw-rgb",
            TextureWorkReq::DrawMandelbrot(_) => "draw-mandelbrot",
            TextureWorkReq::DrawTex(_) => "draw-tex",
        };
        crate::log!(
            "gfx-cabi: texture work queue full cap={} dropped={}\n",
            TEXTURE_UPLOAD_RING_CAP,
            kind
        );
    }

    fn queue_texture_rgba_upload_owned(
        tex_id: u32,
        width: u32,
        height: u32,
        region: Option<ImageRegion>,
        rgba: Vec<u8>,
        sample_kind: TexSampleKind,
        repaint_window_id: u32,
        repaint_reason: &'static str,
        update_async_status: bool,
    ) -> bool {
        if tex_id == 0 || width == 0 || height == 0 {
            return false;
        }
        if reject_unreasonable_tex_id(tex_id, "queue-texture-upload") {
            return false;
        }
        if !claim_current_vm_texture_id(tex_id, "queue-texture-upload-owner") {
            return false;
        }
        let host_tex_id = host_texture_id_for_current_context(tex_id);
        if host_tex_id == 0 || reject_unreasonable_tex_id(host_tex_id, "queue-texture-upload-host")
        {
            return false;
        }
        if checked_reasonable_rgba_len(width, height).is_none() {
            crate::log!(
                "gfx-cabi: reject texture-upload queue tex={} size={}x{} repaint={} window={}\n",
                tex_id,
                width,
                height,
                repaint_reason,
                repaint_window_id
            );
            return false;
        }
        let expected = match region {
            Some(region) => {
                if region.width == 0
                    || region.height == 0
                    || region.x.saturating_add(region.width) > width
                    || region.y.saturating_add(region.height) > height
                {
                    return false;
                }
                (region.width as usize)
                    .saturating_mul(region.height as usize)
                    .saturating_mul(4)
            }
            None => (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(4),
        };
        if rgba.len() < expected {
            return false;
        }
        record_vm_texture_dimensions(tex_id, width, height);
        enqueue_texture_upload(TextureUploadReq {
            tex_id: host_tex_id,
            width,
            height,
            region,
            rgba,
            sample_kind,
            repaint_window_id,
            repaint_reason,
            update_async_status,
        });
        true
    }

    pub fn queue_texture_rgba_image_upload_owned(
        tex_id: u32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        repaint_window_id: u32,
        repaint_reason: &'static str,
    ) -> bool {
        queue_texture_rgba_upload_owned(
            tex_id,
            width,
            height,
            None,
            rgba,
            TexSampleKind::Rgba,
            repaint_window_id,
            repaint_reason,
            false,
        )
    }

    pub fn queue_texture_rgba_image_upload_copy(
        tex_id: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
        repaint_window_id: u32,
        repaint_reason: &'static str,
    ) -> bool {
        queue_texture_rgba_upload_owned(
            tex_id,
            width,
            height,
            None,
            rgba.to_vec(),
            TexSampleKind::Rgba,
            repaint_window_id,
            repaint_reason,
            false,
        )
    }

    pub fn queue_texture_mask_image_upload_copy(
        tex_id: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
        repaint_window_id: u32,
        repaint_reason: &'static str,
    ) -> bool {
        queue_texture_rgba_upload_owned(
            tex_id,
            width,
            height,
            None,
            rgba.to_vec(),
            TexSampleKind::Mask,
            repaint_window_id,
            repaint_reason,
            false,
        )
    }

    pub fn queue_texture_rgba_image_region_upload_copy(
        tex_id: u32,
        texture_width: u32,
        texture_height: u32,
        region_x: u32,
        region_y: u32,
        region_width: u32,
        region_height: u32,
        rgba: &[u8],
        repaint_window_id: u32,
        repaint_reason: &'static str,
    ) -> bool {
        queue_texture_rgba_upload_owned(
            tex_id,
            texture_width,
            texture_height,
            Some(ImageRegion {
                x: region_x,
                y: region_y,
                width: region_width,
                height: region_height,
            }),
            rgba.to_vec(),
            TexSampleKind::Rgba,
            repaint_window_id,
            repaint_reason,
            false,
        )
    }

    pub fn queue_render_rgb_triangles_to_texture_copy(
        tex_id: u32,
        clear_rgb: u32,
        vtx: &[u8],
        repaint_window_id: u32,
        repaint_reason: &'static str,
    ) -> bool {
        if tex_id == 0 {
            return false;
        }
        if reject_unreasonable_tex_id(tex_id, "queue-draw-rgb") {
            return false;
        }
        if !claim_current_vm_texture_id(tex_id, "queue-draw-rgb-owner") {
            return false;
        }
        let host_tex_id = host_texture_id_for_current_context(tex_id);
        if host_tex_id == 0 || reject_unreasonable_tex_id(host_tex_id, "queue-draw-rgb-host") {
            return false;
        }
        if vtx.is_empty() {
            return true;
        }
        const VTX_SIZE: usize = 12;
        let usable = vtx.len() - (vtx.len() % VTX_SIZE);
        if usable == 0 {
            return false;
        }

        enqueue_texture_draw_rgb(TextureDrawRgbReq {
            tex_id: host_tex_id,
            clear_rgb,
            verts: vtx[..usable].to_vec(),
            repaint_window_id,
            repaint_reason,
        });
        true
    }

    pub fn queue_render_mandelbrot_to_texture(
        tex_id: u32,
        ticks: u64,
        tick_hz: u64,
        repaint_window_id: u32,
        repaint_reason: &'static str,
    ) -> bool {
        if tex_id == 0 {
            return false;
        }
        if reject_unreasonable_tex_id(tex_id, "queue-draw-mandelbrot") {
            return false;
        }
        if !claim_current_vm_texture_id(tex_id, "queue-draw-mandelbrot-owner") {
            return false;
        }
        let host_tex_id = host_texture_id_for_current_context(tex_id);
        if host_tex_id == 0 || reject_unreasonable_tex_id(host_tex_id, "queue-draw-mandelbrot-host")
        {
            return false;
        }
        enqueue_texture_draw_mandelbrot(TextureDrawMandelbrotReq {
            tex_id: host_tex_id,
            ticks,
            tick_hz,
            repaint_window_id,
            repaint_reason,
        });
        true
    }

    pub fn queue_render_tex_triangles_to_texture_copy(
        target_tex_id: u32,
        source_tex_id: u32,
        clear_rgb: u32,
        vtx: &[u8],
        repaint_window_id: u32,
        repaint_reason: &'static str,
    ) -> bool {
        if target_tex_id == 0 || source_tex_id == 0 {
            return false;
        }
        if reject_unreasonable_tex_pair(target_tex_id, source_tex_id, "queue-draw-tex") {
            return false;
        }
        if !claim_current_vm_texture_id(target_tex_id, "queue-draw-tex-target-owner") {
            return false;
        }
        if source_tex_id != target_tex_id
            && !require_current_vm_texture_owner(source_tex_id, "queue-draw-tex-source-owner")
        {
            return false;
        }
        let host_target_tex_id = host_texture_id_for_current_context(target_tex_id);
        let host_source_tex_id = host_texture_id_for_current_context(source_tex_id);
        if host_target_tex_id == 0
            || host_source_tex_id == 0
            || reject_unreasonable_tex_pair(
                host_target_tex_id,
                host_source_tex_id,
                "queue-draw-tex-host",
            )
        {
            return false;
        }
        if vtx.is_empty() {
            return true;
        }
        const VTX_SIZE: usize = 20;
        let usable = vtx.len() - (vtx.len() % VTX_SIZE);
        if usable == 0 {
            return false;
        }

        enqueue_texture_draw_tex(TextureDrawTexReq {
            target_tex_id: host_target_tex_id,
            source_tex_id: host_source_tex_id,
            clear_rgb,
            verts: vtx[..usable].to_vec(),
            particle_shader: false,
            repaint_window_id,
            repaint_reason,
        });
        true
    }

    pub fn queue_render_particle_tex_triangles_to_texture_copy(
        target_tex_id: u32,
        source_tex_id: u32,
        clear_rgb: u32,
        vtx: &[u8],
        repaint_window_id: u32,
        repaint_reason: &'static str,
    ) -> bool {
        if target_tex_id == 0 || source_tex_id == 0 {
            return false;
        }
        if reject_unreasonable_tex_pair(target_tex_id, source_tex_id, "queue-draw-particle-tex") {
            return false;
        }
        if !claim_current_vm_texture_id(target_tex_id, "queue-draw-particle-target-owner") {
            return false;
        }
        if source_tex_id != target_tex_id
            && !require_current_vm_texture_owner(source_tex_id, "queue-draw-particle-source-owner")
        {
            return false;
        }
        let host_target_tex_id = host_texture_id_for_current_context(target_tex_id);
        let host_source_tex_id = host_texture_id_for_current_context(source_tex_id);
        if host_target_tex_id == 0
            || host_source_tex_id == 0
            || reject_unreasonable_tex_pair(
                host_target_tex_id,
                host_source_tex_id,
                "queue-draw-particle-tex-host",
            )
        {
            return false;
        }
        if vtx.is_empty() {
            return true;
        }
        const VTX_SIZE: usize = 20;
        let usable = vtx.len() - (vtx.len() % VTX_SIZE);
        if usable == 0 {
            return false;
        }

        enqueue_texture_draw_tex(TextureDrawTexReq {
            target_tex_id: host_target_tex_id,
            source_tex_id: host_source_tex_id,
            clear_rgb,
            verts: vtx[..usable].to_vec(),
            particle_shader: true,
            repaint_window_id,
            repaint_reason,
        });
        true
    }

    async fn texture_upload_service_inner() {
        loop {
            let Some(req) = take_texture_upload() else {
                TEXTURE_UPLOAD_WAIT.wait_for_event().await;
                continue;
            };
            if end_frame_in_progress() {
                log_texture_worker_skipped_end_frame_active();
                requeue_texture_work_front(req);
                Timer::after_millis(1).await;
                continue;
            }
            match req {
                TextureWorkReq::Upload(req) => {
                    let rgba_len = req.rgba.len();
                    if crate::logflag::GFX_CABI_FRAME_DEBUG_LOGS {
                        match req.region {
                            Some(region) => crate::log!(
                                "gfx-cabi: texture-upload tex={} size={}x{} region={}x{}@{},{} rgba_len={} kind={} repaint={} window={}\n",
                                req.tex_id,
                                req.width,
                                req.height,
                                region.width,
                                region.height,
                                region.x,
                                region.y,
                                rgba_len,
                                match req.sample_kind {
                                    TexSampleKind::Mask => "mask",
                                    TexSampleKind::Rgba => "rgba",
                                },
                                req.repaint_reason,
                                req.repaint_window_id
                            ),
                            None => crate::log!(
                                "gfx-cabi: texture-upload tex={} size={}x{} region=full rgba_len={} kind={} repaint={} window={}\n",
                                req.tex_id,
                                req.width,
                                req.height,
                                rgba_len,
                                match req.sample_kind {
                                    TexSampleKind::Mask => "mask",
                                    TexSampleKind::Rgba => "rgba",
                                },
                                req.repaint_reason,
                                req.repaint_window_id
                            ),
                        }
                    }
                    let rc = upload_texture_rgba_inner_owned(
                        req.tex_id,
                        req.width,
                        req.height,
                        req.region,
                        req.rgba,
                        req.sample_kind,
                    );
                    if req.update_async_status {
                        if rc == 0 {
                            set_async_tex_status(req.tex_id, ASYNC_TEX_STATUS_READY);
                        } else {
                            set_async_tex_status(req.tex_id, rc);
                        }
                    }
                    if rc == 0 {
                        request_texture_work_present(
                            req.repaint_window_id,
                            req.tex_id,
                            req.repaint_reason,
                        );
                    } else {
                        log_texture_work_failed(
                            "upload",
                            rc,
                            req.tex_id,
                            0,
                            req.repaint_window_id,
                            req.repaint_reason,
                        );
                    }
                }
                TextureWorkReq::DrawRgb(req) => {
                    let rc = render_rgb_triangles_to_texture_now(
                        req.tex_id,
                        req.clear_rgb,
                        req.verts.as_slice(),
                    );
                    if rc == 0 {
                        request_texture_work_present(
                            req.repaint_window_id,
                            req.tex_id,
                            req.repaint_reason,
                        );
                    } else {
                        log_texture_work_failed(
                            "draw-rgb",
                            rc,
                            req.tex_id,
                            0,
                            req.repaint_window_id,
                            req.repaint_reason,
                        );
                    }
                }
                TextureWorkReq::DrawMandelbrot(req) => {
                    let rc = render_mandelbrot_to_texture_now(req.tex_id, req.ticks, req.tick_hz);
                    if rc == 0 {
                        request_texture_work_present(
                            req.repaint_window_id,
                            req.tex_id,
                            req.repaint_reason,
                        );
                    } else {
                        log_texture_work_failed(
                            "draw-mandelbrot",
                            rc,
                            req.tex_id,
                            0,
                            req.repaint_window_id,
                            req.repaint_reason,
                        );
                    }
                }
                TextureWorkReq::DrawTex(req) => {
                    let rc = render_tex_triangles_to_texture_now(
                        req.target_tex_id,
                        req.source_tex_id,
                        req.clear_rgb,
                        req.verts.as_slice(),
                        req.particle_shader,
                    );
                    if rc == 0 {
                        request_texture_work_present(
                            req.repaint_window_id,
                            req.target_tex_id,
                            req.repaint_reason,
                        );
                    } else {
                        log_texture_work_failed(
                            "draw-tex",
                            rc,
                            req.target_tex_id,
                            req.source_tex_id,
                            req.repaint_window_id,
                            req.repaint_reason,
                        );
                    }
                }
            }
            Timer::after_millis(1).await;
        }
    }

    #[embassy_executor::task]
    pub async fn texture_upload_service_task() {
        let hz = embassy_time_driver::TICK_HZ.max(1);
        let ms = embassy_time_driver::now().saturating_mul(1000) / hz;
        crate::log!("boot-probe: texture-upload task start ms={}\n", ms);
        crate::r::readiness::set(crate::r::readiness::GFX_TEXTURE_UPLOAD_SERVICE_READY);
        texture_upload_service_inner().await;
    }

    async fn async_png_decode_upload_inner(tex_id: u32, bytes: Vec<u8>) {
        let rc = match crate::gfx::png_codec::decode_png_rgba(bytes.as_slice()) {
            Ok(decoded) => {
                if queue_texture_rgba_upload_owned(
                    tex_id,
                    decoded.width,
                    decoded.height,
                    None,
                    decoded.rgba,
                    TexSampleKind::Rgba,
                    0,
                    "",
                    true,
                ) {
                    0
                } else {
                    -5
                }
            }
            Err(err) => err.code(),
        };
        if rc != 0 {
            set_async_tex_status(tex_id, rc);
        }
    }

    #[embassy_executor::task(pool_size = ASYNC_PNG_DECODE_TASK_POOL_SIZE)]
    async fn async_png_decode_upload_task(tex_id: u32, bytes: Vec<u8>) {
        async_png_decode_upload_inner(tex_id, bytes).await;
    }

    #[embassy_executor::task(pool_size = ASYNC_JPEG_DECODE_TASK_POOL_SIZE)]
    async fn async_jpeg_decode_upload_task(tex_id: u32, bytes: Vec<u8>) {
        async_jpeg_decode_upload_inner(tex_id, bytes).await;
        try_spawn_async_jpeg_decode_uploads();
    }

    async fn async_jpeg_decode_upload_inner(tex_id: u32, bytes: Vec<u8>) {
        let rc = match crate::gfx::jpeg_codec::decode_jpeg_rgba(bytes.as_slice()) {
            Ok(decoded) => {
                if queue_texture_rgba_upload_owned(
                    tex_id,
                    decoded.width,
                    decoded.height,
                    None,
                    decoded.rgba,
                    TexSampleKind::Rgba,
                    0,
                    "",
                    true,
                ) {
                    0
                } else {
                    -5
                }
            }
            Err(err) => err.code(),
        };
        if rc != 0 {
            set_async_tex_status(tex_id, rc);
        }
    }

    fn svg_bytes_start_like_svg(data: &[u8]) -> bool {
        let mut offset = 0;
        while offset < data.len() && matches!(data[offset], b' ' | b'\n' | b'\r' | b'\t') {
            offset += 1;
        }
        data.get(offset..)
            .is_some_and(|tail| tail.starts_with(b"<svg"))
    }

    fn log_svg_upload_failure(
        path: &'static str,
        tex_id: u32,
        data_len: usize,
        rc: i32,
        data: Option<&[u8]>,
    ) {
        let task_domain = crate::t::kernel_task_domain::current();
        let starts_svg = data.map(svg_bytes_start_like_svg).unwrap_or(false);
        crate::log!(
            "gfx-cabi: svg upload failed path={} tex={} len={} rc={} starts_svg={} vm_ctx={:?} hull_vm={:?} task_domain={:?}\n",
            path,
            tex_id,
            data_len,
            rc,
            starts_svg as u8,
            crate::hv::current_guest_execution_context_vm_id(),
            crate::hv::current_hull_guest_context_vm_id(),
            task_domain,
        );
    }

    async fn async_svg_decode_upload_inner(tex_id: u32, bytes: Vec<u8>) {
        let data_len = bytes.len();
        let rc = match crate::gfx::svg::rasterize_svg_bytes_rgba(bytes.as_slice()) {
            Ok((info, rgba)) => {
                if queue_texture_rgba_upload_owned(
                    tex_id,
                    info.width,
                    info.height,
                    None,
                    rgba,
                    TexSampleKind::Rgba,
                    0,
                    "",
                    true,
                ) {
                    0
                } else {
                    -5
                }
            }
            Err(code) => code,
        };
        if rc != 0 {
            log_svg_upload_failure("async-svg", tex_id, data_len, rc, None);
            set_async_tex_status(tex_id, rc);
        }
    }

    async fn async_svg_upload_service_inner() {
        loop {
            let Some(req) = take_async_svg_upload() else {
                ASYNC_SVG_WAIT.wait_for_event().await;
                continue;
            };
            async_svg_decode_upload_inner(req.tex_id, req.bytes).await;
            Timer::after_millis(1).await;
        }
    }

    #[embassy_executor::task]
    async fn async_svg_upload_service_task() {
        async_svg_upload_service_inner().await;
    }

    fn spawn_async_png_decode_upload(tex_id: u32, bytes: Vec<u8>) -> i32 {
        let Some((slot, kind, spawner)) = crate::workers::pick_background_spawner_with_slot()
        else {
            crate::globalog::log(format_args!(
                "async-png: no background spawner available; decode not queued tex={}\n",
                tex_id
            ));
            return -4;
        };

        let Ok(token) = async_png_decode_upload_task(tex_id, bytes) else {
            crate::globalog::log(format_args!(
                "async-png: decode task pool exhausted tex={}\n",
                tex_id
            ));
            return -4;
        };

        set_async_tex_status(tex_id, ASYNC_TEX_STATUS_PENDING);
        spawner.spawn(token);
        crate::globalog::log(format_args!(
            "async-png: decode task queued tex={} slot={} core_kind={}\n",
            tex_id, slot, kind
        ));
        0
    }

    fn try_spawn_async_jpeg_decode_uploads() {
        loop {
            let Some(req) = take_async_jpeg_upload() else {
                return;
            };
            let AsyncJpegUploadReq { tex_id, bytes } = req;

            if let Some((slot, kind, spawner)) = crate::workers::pick_background_spawner_with_slot()
            {
                let Ok(token) = async_jpeg_decode_upload_task(tex_id, bytes.clone()) else {
                    requeue_async_jpeg_upload_front(AsyncJpegUploadReq { tex_id, bytes });
                    return;
                };
                spawner.spawn(token);
                crate::globalog::log(format_args!(
                    "async-jpeg: decode task queued tex={} slot={} core_kind={}\n",
                    tex_id, slot, kind
                ));
                continue;
            }

            if crate::smp::cpu_count() > 1 {
                requeue_async_jpeg_upload_front(AsyncJpegUploadReq { tex_id, bytes });
                crate::globalog::log(format_args!(
                    "async-jpeg: no background spawner available; decode remains queued tex={}\n",
                    tex_id
                ));
                return;
            }

            crate::wait::spawn_local_detached(async move {
                async_jpeg_decode_upload_inner(tex_id, bytes).await;
                try_spawn_async_jpeg_decode_uploads();
            });
        }
    }

    fn try_start_async_svg_worker() {
        if ASYNC_SVG_WORKER_STARTED.load(core::sync::atomic::Ordering::Acquire) {
            return;
        }

        if let Some(worker_spawner) = crate::workers::pick_background_spawner() {
            if ASYNC_SVG_WORKER_STARTED
                .compare_exchange(
                    false,
                    true,
                    core::sync::atomic::Ordering::AcqRel,
                    core::sync::atomic::Ordering::Acquire,
                )
                .is_ok()
            {
                if let Ok(token) = async_svg_upload_service_task() {
                    worker_spawner.spawn(token);
                } else {
                    ASYNC_SVG_WORKER_STARTED.store(false, core::sync::atomic::Ordering::Release);
                }
            }
            return;
        }

        if crate::smp::cpu_count() > 1 {
            crate::globalog::log(format_args!(
                "async-svg: no background spawner available on multicore system; worker not started\n"
            ));
        }

        if crate::smp::cpu_count() <= 1
            && ASYNC_SVG_WORKER_STARTED
                .compare_exchange(
                    false,
                    true,
                    core::sync::atomic::Ordering::AcqRel,
                    core::sync::atomic::Ordering::Acquire,
                )
                .is_ok()
        {
            crate::wait::spawn_local_detached(async move {
                async_svg_upload_service_inner().await;
            });
        }
    }

    struct GfxCabiState {
        pipeline: PipelineId,
        ring_idx: usize,
        vbuf: [BufferId; GFX_CABI_VBUF_RING_LEN],
        capacity: [usize; GFX_CABI_VBUF_RING_LEN],
        tex_pipeline_mask: PipelineId,
        tex_pipeline_rgba: PipelineId,
        tex_pipeline_particle: PipelineId,
        tex_pipeline_mandelbrot: PipelineId,
        tex_vbuf: [BufferId; GFX_CABI_VBUF_RING_LEN],
        tex_capacity: [usize; GFX_CABI_VBUF_RING_LEN],
        tex_images: Option<Vec<Option<TexImage>>>,
        epoch: u64,
        swapchain_configured: bool,
        swapchain_desc: SwapchainDesc,
        viewport_configured: bool,
        frame_active: bool,
        frame_allow_screen_present: bool,
        frame_preserve_contents: bool,
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
        base_cache_screen_width: u32,
        base_cache_screen_height: u32,
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
        // Active render target texture id for the current frame; 0 means swapchain.
        frame_render_target_tex_id: u32,
    }

    #[derive(Clone, Copy)]
    struct ScissorRect {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TexSampleKind {
        Mask,
        Rgba,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TexPipelineKind {
        Mask,
        Rgba,
        Particle,
        Mandelbrot,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TexCoordOrigin {
        TopLeft,
        BottomLeft,
    }

    #[derive(Clone)]
    struct TexImage {
        image: ImageId,
        width: u32,
        height: u32,
        sample_kind: TexSampleKind,
        origin: TexCoordOrigin,
        rgba: Vec<u8>,
    }

    #[derive(Clone, Copy)]
    enum PendingDraw {
        SetRenderTarget {
            tex_id: u32,
        },
        SetScissor {
            rect: Option<ScissorRect>,
        },
        ClearRect {
            rgb: u32,
            x: u32,
            y: u32,
            width: u32,
            height: u32,
        },
        Rgb {
            blob_offset: usize,
            blob_len: usize,
            blend: BlendDesc,
        },
        Tex {
            tex_id: u32,
            image: ImageId,
            sample_kind: TexSampleKind,
            sampler: SamplerDesc,
            blob_offset: usize,
            blob_len: usize,
            blend: BlendDesc,
        },
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct TrueosGfxTraceEntry {
        pub seq: u32,
        pub op: u32,
        pub frame_seq: u32,
        pub flags: u32,
        pub a: u32,
        pub b: u32,
        pub c: u32,
        pub d: u32,
    }

    const GFX_TRACE_CAPACITY: usize = 1024;

    const GFX_TRACE_OP_BEGIN_FRAME: u32 = 1;
    const GFX_TRACE_OP_END_FRAME: u32 = 2;
    const GFX_TRACE_OP_SET_BLEND: u32 = 3;
    const GFX_TRACE_OP_SET_SAMPLER: u32 = 4;
    const GFX_TRACE_OP_SET_SCISSOR: u32 = 5;
    const GFX_TRACE_OP_CLEAR_SCISSOR: u32 = 6;
    const GFX_TRACE_OP_SET_RENDER_TARGET: u32 = 7;
    const GFX_TRACE_OP_CLEAR_RENDER_TARGET: u32 = 8;
    const GFX_TRACE_OP_UPLOAD_TEXTURE_RGBA: u32 = 9;
    const GFX_TRACE_OP_UPLOAD_TEXTURE_PNG: u32 = 10;
    const GFX_TRACE_OP_UPLOAD_TEXTURE_JPEG: u32 = 11;
    const GFX_TRACE_OP_UPLOAD_TEXTURE_SVG: u32 = 12;
    const GFX_TRACE_OP_DRAW_RGB_TRIANGLES: u32 = 13;
    const GFX_TRACE_OP_DRAW_TEX_TRIANGLES: u32 = 14;
    const GFX_TRACE_OP_CLEAR_RECT: u32 = 15;

    struct GfxTraceRing {
        enabled: bool,
        head: usize,
        len: usize,
        next_seq: u32,
        dropped: u32,
        entries: [TrueosGfxTraceEntry; GFX_TRACE_CAPACITY],
    }

    impl GfxTraceRing {
        const fn new() -> Self {
            Self {
                enabled: false,
                head: 0,
                len: 0,
                next_seq: 1,
                dropped: 0,
                entries: [TrueosGfxTraceEntry {
                    seq: 0,
                    op: 0,
                    frame_seq: 0,
                    flags: 0,
                    a: 0,
                    b: 0,
                    c: 0,
                    d: 0,
                }; GFX_TRACE_CAPACITY],
            }
        }
    }

    static GFX_TRACE_RING: spin::Mutex<GfxTraceRing> = spin::Mutex::new(GfxTraceRing::new());
    static END_FRAME_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
    static CURSOR_HELPER_SKIP_LOGS: AtomicU32 = AtomicU32::new(0);
    static SCREENSHOT_HELPER_SKIP_LOGS: AtomicU32 = AtomicU32::new(0);
    static TEXTURE_WORKER_SKIP_LOGS: AtomicU32 = AtomicU32::new(0);

    #[inline]
    fn gfx_trace_record(op: u32, frame_seq: u32, flags: u32, a: u32, b: u32, c: u32, d: u32) {
        let mut ring = GFX_TRACE_RING.lock();
        if !ring.enabled {
            return;
        }
        let entry = TrueosGfxTraceEntry {
            seq: ring.next_seq,
            op,
            frame_seq,
            flags,
            a,
            b,
            c,
            d,
        };
        ring.next_seq = ring.next_seq.wrapping_add(1);
        let head = ring.head;
        ring.entries[head] = entry;
        ring.head = (head + 1) % GFX_TRACE_CAPACITY;
        if ring.len < GFX_TRACE_CAPACITY {
            ring.len += 1;
        } else {
            ring.dropped = ring.dropped.saturating_add(1);
        }
    }

    struct EndFrameProgressGuard;

    impl EndFrameProgressGuard {
        #[inline]
        fn new() -> Self {
            END_FRAME_IN_PROGRESS.store(true, Ordering::Release);
            Self
        }
    }

    impl Drop for EndFrameProgressGuard {
        fn drop(&mut self) {
            END_FRAME_IN_PROGRESS.store(false, Ordering::Release);
        }
    }

    #[inline]
    fn end_frame_in_progress() -> bool {
        END_FRAME_IN_PROGRESS.load(Ordering::Acquire)
    }

    #[inline]
    fn log_cursor_helper_skipped_end_frame_active() {
        let n = CURSOR_HELPER_SKIP_LOGS.fetch_add(1, Ordering::Relaxed);
        if n < 32 {
            crate::globalog::log(format_args!(
                "gfx-cabi: cursor overlay tick skipped because end_frame is active\n"
            ));
        }
    }

    #[inline]
    fn log_screenshot_helper_skipped_end_frame_active() {
        let n = SCREENSHOT_HELPER_SKIP_LOGS.fetch_add(1, Ordering::Relaxed);
        if n < 32 {
            crate::globalog::log(format_args!(
                "gfx-cabi: composed screenshot helper skipped because end_frame is active\n"
            ));
        }
    }

    #[inline]
    fn log_texture_worker_skipped_end_frame_active() {
        let n = TEXTURE_WORKER_SKIP_LOGS.fetch_add(1, Ordering::Relaxed);
        if n < 32 {
            crate::globalog::log(format_args!(
                "gfx-cabi: texture worker deferred because end_frame is active\n"
            ));
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_trace_set_enabled(enabled: u32) -> u32 {
        if gfx_cabi_vm_context() {
            return 0;
        }
        let mut ring = GFX_TRACE_RING.lock();
        let prev = ring.enabled;
        ring.enabled = enabled != 0;
        prev as u32
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_trace_clear() {
        if gfx_cabi_vm_context() {
            return;
        }
        let mut ring = GFX_TRACE_RING.lock();
        ring.head = 0;
        ring.len = 0;
        ring.dropped = 0;
        ring.next_seq = 1;
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_trace_snapshot(
        out_ptr: *mut TrueosGfxTraceEntry,
        out_cap: u32,
    ) -> u32 {
        if gfx_cabi_vm_context() {
            return 0;
        }
        let ring = GFX_TRACE_RING.lock();
        if out_cap == 0 || out_ptr.is_null() {
            return ring.len.min(out_cap as usize) as u32;
        }
        let want = ring.len.min(out_cap as usize);
        let start = (ring.head + GFX_TRACE_CAPACITY - want) % GFX_TRACE_CAPACITY;
        for i in 0..want {
            let idx = (start + i) % GFX_TRACE_CAPACITY;
            unsafe {
                core::ptr::write(out_ptr.add(i), ring.entries[idx]);
            }
        }
        want as u32
    }

    mod io_cursor {
        use super::*;

        include!("io_cursor.rs");
    }

    pub use io_cursor::*;

    impl GfxCabiState {
        const fn new() -> Self {
            Self {
                pipeline: PipelineId::invalid(),
                ring_idx: 0,
                vbuf: [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN],
                capacity: [0; GFX_CABI_VBUF_RING_LEN],
                tex_pipeline_mask: PipelineId::invalid(),
                tex_pipeline_rgba: PipelineId::invalid(),
                tex_pipeline_particle: PipelineId::invalid(),
                tex_pipeline_mandelbrot: PipelineId::invalid(),
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
                frame_allow_screen_present: true,
                frame_preserve_contents: false,
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
                base_cache_screen_width: 0,
                base_cache_screen_height: 0,
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
                frame_render_target_tex_id: 0,
            }
        }
    }

    #[inline]
    fn append_tex_vertices_with_origin(out: &mut Vec<u8>, src: &[u8], origin: TexCoordOrigin) {
        const VTX_SIZE: usize = 20;
        if origin == TexCoordOrigin::TopLeft {
            out.extend_from_slice(src);
            return;
        }

        let mut off = 0usize;
        while off + VTX_SIZE <= src.len() {
            out.extend_from_slice(&src[off..off + 12]);
            let mut v_bytes = [0u8; 4];
            v_bytes.copy_from_slice(&src[off + 12..off + 16]);
            let v = f32::from_le_bytes(v_bytes);
            out.extend_from_slice(&(1.0f32 - v).to_le_bytes());
            out.extend_from_slice(&src[off + 16..off + VTX_SIZE]);
            off += VTX_SIZE;
        }
    }

    fn texture_dimensions_inner(tex_id: u32) -> Option<(u32, u32)> {
        if tex_id == 0 {
            return None;
        }
        if reject_unreasonable_tex_id(tex_id, "texture-dimensions") {
            return None;
        }
        let idx = tex_id.saturating_sub(1) as usize;
        GFX_CABI_STATE
            .lock()
            .tex_images
            .as_ref()
            .and_then(|images| images.get(idx))
            .and_then(|entry| entry.as_ref())
            .map(|img| (img.width, img.height))
    }

    pub fn host_texture_has_image(tex_id: u32) -> bool {
        texture_dimensions_inner(tex_id).is_some()
    }

    #[inline]
    fn clear_rgba_buffer(rgba: &mut [u8], rgb: u32) {
        let r = ((rgb >> 16) & 0xFF) as u8;
        let g = ((rgb >> 8) & 0xFF) as u8;
        let b = (rgb & 0xFF) as u8;
        for px in rgba.chunks_exact_mut(4) {
            px[0] = r;
            px[1] = g;
            px[2] = b;
            px[3] = 255;
        }
    }

    #[inline]
    fn checked_rgba_len(width: u32, height: u32) -> Option<usize> {
        let pixels = (width as usize).checked_mul(height as usize)?;
        pixels.checked_mul(4)
    }

    const MAX_SHARED_TEXTURE_DIM: u32 = 8192;
    const MAX_SHARED_TEXTURE_BYTES: usize = 256 * 1024 * 1024;

    #[inline]
    fn checked_reasonable_rgba_len(width: u32, height: u32) -> Option<usize> {
        let len = checked_rgba_len(width, height)?;
        if width > MAX_SHARED_TEXTURE_DIM
            || height > MAX_SHARED_TEXTURE_DIM
            || len > MAX_SHARED_TEXTURE_BYTES
        {
            return None;
        }
        Some(len)
    }

    #[inline]
    fn clear_rgba_rect(
        rgba: &mut [u8],
        width: u32,
        height: u32,
        x: u32,
        y: u32,
        rect_w: u32,
        rect_h: u32,
        rgb: u32,
    ) {
        if width == 0 || height == 0 || rect_w == 0 || rect_h == 0 {
            return;
        }

        let x0 = x.min(width) as usize;
        let y0 = y.min(height) as usize;
        let x1 = x.saturating_add(rect_w).min(width) as usize;
        let y1 = y.saturating_add(rect_h).min(height) as usize;
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        let r = ((rgb >> 16) & 0xFF) as u8;
        let g = ((rgb >> 8) & 0xFF) as u8;
        let b = (rgb & 0xFF) as u8;
        let stride = width as usize * 4;
        for py in y0..y1 {
            let row = py.saturating_mul(stride);
            for px in x0..x1 {
                let off = row + px.saturating_mul(4);
                if off + 4 > rgba.len() {
                    break;
                }
                rgba[off] = r;
                rgba[off + 1] = g;
                rgba[off + 2] = b;
                rgba[off + 3] = 255;
            }
        }
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
    fn pixel_to_rgba(px: &[u8]) -> [f32; 4] {
        [
            (px[0] as f32) / 255.0,
            (px[1] as f32) / 255.0,
            (px[2] as f32) / 255.0,
            (px[3] as f32) / 255.0,
        ]
    }

    #[inline]
    fn write_rgba_pixel(dst: &mut [u8], rgba: [f32; 4]) {
        dst[0] = (clamp01(rgba[0]) * 255.0 + 0.5) as u8;
        dst[1] = (clamp01(rgba[1]) * 255.0 + 0.5) as u8;
        dst[2] = (clamp01(rgba[2]) * 255.0 + 0.5) as u8;
        dst[3] = (clamp01(rgba[3]) * 255.0 + 0.5) as u8;
    }

    #[inline]
    fn ndc_to_target_x(x: f32, width: u32) -> f32 {
        ((x + 1.0) * 0.5) * width as f32
    }

    #[inline]
    fn ndc_to_target_y(y: f32, height: u32) -> f32 {
        ((1.0 - y) * 0.5) * height as f32
    }

    #[inline]
    fn edge_fn(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
        (px - ax) * (by - ay) - (py - ay) * (bx - ax)
    }

    #[inline]
    fn blend_factor_rgba(factor: BlendFactor, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
        match factor {
            BlendFactor::Zero => [0.0, 0.0, 0.0, 0.0],
            BlendFactor::One => [1.0, 1.0, 1.0, 1.0],
            BlendFactor::SrcAlpha => [src[3], src[3], src[3], src[3]],
            BlendFactor::OneMinusSrcAlpha => {
                let v = 1.0 - src[3];
                [v, v, v, v]
            }
            BlendFactor::DstColor => dst,
            BlendFactor::OneMinusDstColor => {
                [1.0 - dst[0], 1.0 - dst[1], 1.0 - dst[2], 1.0 - dst[3]]
            }
            BlendFactor::OneMinusSrcColor => {
                [1.0 - src[0], 1.0 - src[1], 1.0 - src[2], 1.0 - src[3]]
            }
        }
    }

    #[inline]
    fn blend_pixel(dst: &mut [u8], src: [f32; 4], blend: BlendDesc) {
        if !blend.enabled {
            write_rgba_pixel(dst, src);
            return;
        }

        let dst_rgba = pixel_to_rgba(dst);
        let src_factor = blend_factor_rgba(blend.src, src, dst_rgba);
        let dst_factor = blend_factor_rgba(blend.dst, src, dst_rgba);
        let out = [
            src[0] * src_factor[0] + dst_rgba[0] * dst_factor[0],
            src[1] * src_factor[1] + dst_rgba[1] * dst_factor[1],
            src[2] * src_factor[2] + dst_rgba[2] * dst_factor[2],
            src[3] * src_factor[3] + dst_rgba[3] * dst_factor[3],
        ];
        write_rgba_pixel(dst, out);
    }

    #[inline]
    fn wrap_tex_coord(coord: f32, wrap: SamplerWrap) -> f32 {
        match wrap {
            SamplerWrap::ClampToEdge => clamp01(coord),
            SamplerWrap::Repeat => {
                let wrapped = coord - libm::floorf(coord);
                if wrapped < 0.0 {
                    wrapped + 1.0
                } else {
                    wrapped
                }
            }
        }
    }

    #[inline]
    fn sample_texel_clamped(rgba: &[u8], width: u32, height: u32, x: i32, y: i32) -> [f32; 4] {
        if width == 0 || height == 0 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let xi = x.clamp(0, width.saturating_sub(1) as i32) as usize;
        let yi = y.clamp(0, height.saturating_sub(1) as i32) as usize;
        let idx = yi
            .saturating_mul(width as usize)
            .saturating_add(xi)
            .saturating_mul(4);
        if idx + 4 > rgba.len() {
            return [0.0, 0.0, 0.0, 0.0];
        }
        pixel_to_rgba(&rgba[idx..idx + 4])
    }

    fn sample_texture_rgba(tex: &TexImage, sampler: SamplerDesc, u: f32, v: f32) -> [f32; 4] {
        if tex.width == 0 || tex.height == 0 || tex.rgba.len() < 4 {
            return [0.0, 0.0, 0.0, 0.0];
        }

        let u = wrap_tex_coord(u, sampler.wrap_s);
        let v = wrap_tex_coord(v, sampler.wrap_t);
        let width_f = tex.width as f32;
        let height_f = tex.height as f32;

        if sampler.min_filter == SamplerFilter::Nearest
            && sampler.mag_filter == SamplerFilter::Nearest
        {
            let x = libm::floorf((u * width_f).min(width_f - 1.0)) as i32;
            let y = libm::floorf((v * height_f).min(height_f - 1.0)) as i32;
            return sample_texel_clamped(&tex.rgba, tex.width, tex.height, x, y);
        }

        let fx = u * width_f - 0.5;
        let fy = v * height_f - 0.5;
        let x0 = libm::floorf(fx) as i32;
        let y0 = libm::floorf(fy) as i32;
        let x1 = x0 + 1;
        let y1 = y0 + 1;
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;
        let c00 = sample_texel_clamped(&tex.rgba, tex.width, tex.height, x0, y0);
        let c10 = sample_texel_clamped(&tex.rgba, tex.width, tex.height, x1, y0);
        let c01 = sample_texel_clamped(&tex.rgba, tex.width, tex.height, x0, y1);
        let c11 = sample_texel_clamped(&tex.rgba, tex.width, tex.height, x1, y1);

        let mut out = [0.0; 4];
        for i in 0..4 {
            let top = lerp(c00[i], c10[i], tx);
            let bot = lerp(c01[i], c11[i], tx);
            out[i] = lerp(top, bot, ty);
        }
        out
    }

    fn draw_rgb_triangle_rgba(
        target: &mut [u8],
        width: u32,
        height: u32,
        scissor: Option<ScissorRect>,
        blend: BlendDesc,
        v0: RgbVtx,
        v1: RgbVtx,
        v2: RgbVtx,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        let p0 = (ndc_to_target_x(v0.x, width), ndc_to_target_y(v0.y, height));
        let p1 = (ndc_to_target_x(v1.x, width), ndc_to_target_y(v1.y, height));
        let p2 = (ndc_to_target_x(v2.x, width), ndc_to_target_y(v2.y, height));
        let area = edge_fn(p0.0, p0.1, p1.0, p1.1, p2.0, p2.1);
        if area.abs() <= 1e-6 {
            return;
        }

        let mut min_x = libm::floorf(p0.0.min(p1.0).min(p2.0)).max(0.0) as i32;
        let mut max_x = libm::ceilf(p0.0.max(p1.0).max(p2.0)).min(width as f32) as i32;
        let mut min_y = libm::floorf(p0.1.min(p1.1).min(p2.1)).max(0.0) as i32;
        let mut max_y = libm::ceilf(p0.1.max(p1.1).max(p2.1)).min(height as f32) as i32;
        if let Some(scissor) = scissor {
            min_x = min_x.max(scissor.x.min(width) as i32);
            max_x = max_x.min(scissor.x.saturating_add(scissor.width).min(width) as i32);
            min_y = min_y.max(scissor.y.min(height) as i32);
            max_y = max_y.min(scissor.y.saturating_add(scissor.height).min(height) as i32);
        }
        if min_x >= max_x || min_y >= max_y {
            return;
        }

        for y in min_y..max_y {
            for x in min_x..max_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let w0 = edge_fn(p1.0, p1.1, p2.0, p2.1, px, py);
                let w1 = edge_fn(p2.0, p2.1, p0.0, p0.1, px, py);
                let w2 = edge_fn(p0.0, p0.1, p1.0, p1.1, px, py);
                if (area > 0.0 && (w0 < 0.0 || w1 < 0.0 || w2 < 0.0))
                    || (area < 0.0 && (w0 > 0.0 || w1 > 0.0 || w2 > 0.0))
                {
                    continue;
                }

                let inv_area = 1.0 / area;
                let b0 = w0 * inv_area;
                let b1 = w1 * inv_area;
                let b2 = w2 * inv_area;
                let src = [
                    v0.r * b0 + v1.r * b1 + v2.r * b2,
                    v0.g * b0 + v1.g * b1 + v2.g * b2,
                    v0.b * b0 + v1.b * b1 + v2.b * b2,
                    v0.a * b0 + v1.a * b1 + v2.a * b2,
                ];

                let idx = (y as usize)
                    .saturating_mul(width as usize)
                    .saturating_add(x as usize)
                    .saturating_mul(4);
                if idx + 4 <= target.len() {
                    blend_pixel(&mut target[idx..idx + 4], src, blend);
                }
            }
        }
    }

    fn draw_tex_triangle_rgba(
        target: &mut [u8],
        width: u32,
        height: u32,
        scissor: Option<ScissorRect>,
        blend: BlendDesc,
        sampler: SamplerDesc,
        sample_kind: TexSampleKind,
        texture: &TexImage,
        v0: TexVtx,
        v1: TexVtx,
        v2: TexVtx,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        let p0 = (ndc_to_target_x(v0.x, width), ndc_to_target_y(v0.y, height));
        let p1 = (ndc_to_target_x(v1.x, width), ndc_to_target_y(v1.y, height));
        let p2 = (ndc_to_target_x(v2.x, width), ndc_to_target_y(v2.y, height));
        let area = edge_fn(p0.0, p0.1, p1.0, p1.1, p2.0, p2.1);
        if area.abs() <= 1e-6 {
            return;
        }

        let mut min_x = libm::floorf(p0.0.min(p1.0).min(p2.0)).max(0.0) as i32;
        let mut max_x = libm::ceilf(p0.0.max(p1.0).max(p2.0)).min(width as f32) as i32;
        let mut min_y = libm::floorf(p0.1.min(p1.1).min(p2.1)).max(0.0) as i32;
        let mut max_y = libm::ceilf(p0.1.max(p1.1).max(p2.1)).min(height as f32) as i32;
        if let Some(scissor) = scissor {
            min_x = min_x.max(scissor.x.min(width) as i32);
            max_x = max_x.min(scissor.x.saturating_add(scissor.width).min(width) as i32);
            min_y = min_y.max(scissor.y.min(height) as i32);
            max_y = max_y.min(scissor.y.saturating_add(scissor.height).min(height) as i32);
        }
        if min_x >= max_x || min_y >= max_y {
            return;
        }

        for y in min_y..max_y {
            for x in min_x..max_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let w0 = edge_fn(p1.0, p1.1, p2.0, p2.1, px, py);
                let w1 = edge_fn(p2.0, p2.1, p0.0, p0.1, px, py);
                let w2 = edge_fn(p0.0, p0.1, p1.0, p1.1, px, py);
                if (area > 0.0 && (w0 < 0.0 || w1 < 0.0 || w2 < 0.0))
                    || (area < 0.0 && (w0 > 0.0 || w1 > 0.0 || w2 > 0.0))
                {
                    continue;
                }

                let inv_area = 1.0 / area;
                let b0 = w0 * inv_area;
                let b1 = w1 * inv_area;
                let b2 = w2 * inv_area;
                let u = v0.u * b0 + v1.u * b1 + v2.u * b2;
                let v = v0.v * b0 + v1.v * b1 + v2.v * b2;
                let vert = [
                    v0.r * b0 + v1.r * b1 + v2.r * b2,
                    v0.g * b0 + v1.g * b1 + v2.g * b2,
                    v0.b * b0 + v1.b * b1 + v2.b * b2,
                    v0.a * b0 + v1.a * b1 + v2.a * b2,
                ];
                let tex = sample_texture_rgba(texture, sampler, u, v);
                let mask = if tex[3] < 1.0 { tex[3] } else { tex[0] };
                let src = match sample_kind {
                    TexSampleKind::Mask => [
                        vert[0] * mask,
                        vert[1] * mask,
                        vert[2] * mask,
                        vert[3] * mask,
                    ],
                    TexSampleKind::Rgba => [
                        tex[0] * vert[0],
                        tex[1] * vert[1],
                        tex[2] * vert[2],
                        tex[3] * vert[3],
                    ],
                };

                let idx = (y as usize)
                    .saturating_mul(width as usize)
                    .saturating_add(x as usize)
                    .saturating_mul(4);
                if idx + 4 <= target.len() {
                    blend_pixel(&mut target[idx..idx + 4], src, blend);
                }
            }
        }
    }

    fn maybe_publish_composed_screenshot(
        preserve_contents: bool,
        clear_rgb: u32,
        draws: &[PendingDraw],
        rgb_src: &[u8],
        tex_src: &[u8],
    ) {
        if end_frame_in_progress() {
            log_screenshot_helper_skipped_end_frame_active();
            return;
        }

        if !crate::gfx::screenshot_capture_armed() {
            return;
        }

        let (screen_w, screen_h, mut textures) = {
            let st = GFX_CABI_STATE.lock();
            (
                st.swapchain_desc.extent.width,
                st.swapchain_desc.extent.height,
                st.tex_images.clone().unwrap_or_default(),
            )
        };
        if screen_w == 0 || screen_h == 0 {
            return;
        }

        let mut screen = vec![
            0u8;
            (screen_w as usize)
                .saturating_mul(screen_h as usize)
                .saturating_mul(4)
        ];
        if !preserve_contents {
            clear_rgba_buffer(screen.as_mut_slice(), clear_rgb);
        }

        let mut current_target_tex_id = 0u32;
        let mut current_scissor: Option<ScissorRect> = None;
        let mut saw_draw = false;
        let mut cleared_first_target = preserve_contents;

        for draw in draws {
            match *draw {
                PendingDraw::SetRenderTarget { tex_id } => {
                    current_target_tex_id = tex_id;
                }
                PendingDraw::SetScissor { rect } => {
                    current_scissor = rect;
                }
                PendingDraw::ClearRect {
                    rgb,
                    x,
                    y,
                    width,
                    height,
                } => {
                    if current_target_tex_id == 0 {
                        clear_rgba_rect(
                            screen.as_mut_slice(),
                            screen_w,
                            screen_h,
                            x,
                            y,
                            width,
                            height,
                            rgb,
                        );
                    } else if let Some(Some(target)) =
                        textures.get_mut(current_target_tex_id.saturating_sub(1) as usize)
                    {
                        clear_rgba_rect(
                            target.rgba.as_mut_slice(),
                            target.width,
                            target.height,
                            x,
                            y,
                            width,
                            height,
                            rgb,
                        );
                    }
                    saw_draw = true;
                }
                PendingDraw::Rgb {
                    blob_offset,
                    blob_len,
                    blend,
                } => {
                    if blob_offset.saturating_add(blob_len) > rgb_src.len() {
                        continue;
                    }
                    if !cleared_first_target {
                        if current_target_tex_id == 0 {
                            clear_rgba_buffer(screen.as_mut_slice(), clear_rgb);
                        } else if let Some(Some(target)) =
                            textures.get_mut(current_target_tex_id.saturating_sub(1) as usize)
                        {
                            clear_rgba_buffer(target.rgba.as_mut_slice(), clear_rgb);
                        }
                        cleared_first_target = true;
                    }
                    let verts = &rgb_src[blob_offset..blob_offset + blob_len];
                    if current_target_tex_id == 0 {
                        let mut off = 0usize;
                        while off + (3 * RGB_VERTEX_SIZE) <= verts.len() {
                            let Some(v0) = read_rgb_vtx(verts, off) else {
                                break;
                            };
                            let Some(v1) = read_rgb_vtx(verts, off + RGB_VERTEX_SIZE) else {
                                break;
                            };
                            let Some(v2) = read_rgb_vtx(verts, off + (2 * RGB_VERTEX_SIZE)) else {
                                break;
                            };
                            draw_rgb_triangle_rgba(
                                screen.as_mut_slice(),
                                screen_w,
                                screen_h,
                                current_scissor,
                                blend,
                                v0,
                                v1,
                                v2,
                            );
                            off += 3 * RGB_VERTEX_SIZE;
                        }
                    } else if let Some(Some(target)) =
                        textures.get_mut(current_target_tex_id.saturating_sub(1) as usize)
                    {
                        let mut off = 0usize;
                        while off + (3 * RGB_VERTEX_SIZE) <= verts.len() {
                            let Some(v0) = read_rgb_vtx(verts, off) else {
                                break;
                            };
                            let Some(v1) = read_rgb_vtx(verts, off + RGB_VERTEX_SIZE) else {
                                break;
                            };
                            let Some(v2) = read_rgb_vtx(verts, off + (2 * RGB_VERTEX_SIZE)) else {
                                break;
                            };
                            draw_rgb_triangle_rgba(
                                target.rgba.as_mut_slice(),
                                target.width,
                                target.height,
                                current_scissor,
                                blend,
                                v0,
                                v1,
                                v2,
                            );
                            off += 3 * RGB_VERTEX_SIZE;
                        }
                    }
                    saw_draw = true;
                }
                PendingDraw::Tex {
                    tex_id,
                    sample_kind,
                    sampler,
                    blob_offset,
                    blob_len,
                    blend,
                    ..
                } => {
                    if blob_offset.saturating_add(blob_len) > tex_src.len() {
                        continue;
                    }
                    let Some(source_tex) = textures
                        .get(tex_id.saturating_sub(1) as usize)
                        .and_then(|entry| entry.as_ref())
                        .cloned()
                    else {
                        continue;
                    };
                    if !cleared_first_target {
                        if current_target_tex_id == 0 {
                            clear_rgba_buffer(screen.as_mut_slice(), clear_rgb);
                        } else if let Some(Some(target)) =
                            textures.get_mut(current_target_tex_id.saturating_sub(1) as usize)
                        {
                            clear_rgba_buffer(target.rgba.as_mut_slice(), clear_rgb);
                        }
                        cleared_first_target = true;
                    }
                    let verts = &tex_src[blob_offset..blob_offset + blob_len];
                    if current_target_tex_id == 0 {
                        let mut off = 0usize;
                        while off + (3 * TEX_VERTEX_SIZE) <= verts.len() {
                            let Some(v0) = read_tex_vtx(verts, off) else {
                                break;
                            };
                            let Some(v1) = read_tex_vtx(verts, off + TEX_VERTEX_SIZE) else {
                                break;
                            };
                            let Some(v2) = read_tex_vtx(verts, off + (2 * TEX_VERTEX_SIZE)) else {
                                break;
                            };
                            draw_tex_triangle_rgba(
                                screen.as_mut_slice(),
                                screen_w,
                                screen_h,
                                current_scissor,
                                blend,
                                sampler,
                                sample_kind,
                                &source_tex,
                                v0,
                                v1,
                                v2,
                            );
                            off += 3 * TEX_VERTEX_SIZE;
                        }
                    } else if let Some(Some(target)) =
                        textures.get_mut(current_target_tex_id.saturating_sub(1) as usize)
                    {
                        let mut off = 0usize;
                        while off + (3 * TEX_VERTEX_SIZE) <= verts.len() {
                            let Some(v0) = read_tex_vtx(verts, off) else {
                                break;
                            };
                            let Some(v1) = read_tex_vtx(verts, off + TEX_VERTEX_SIZE) else {
                                break;
                            };
                            let Some(v2) = read_tex_vtx(verts, off + (2 * TEX_VERTEX_SIZE)) else {
                                break;
                            };
                            draw_tex_triangle_rgba(
                                target.rgba.as_mut_slice(),
                                target.width,
                                target.height,
                                current_scissor,
                                blend,
                                sampler,
                                sample_kind,
                                &source_tex,
                                v0,
                                v1,
                                v2,
                            );
                            off += 3 * TEX_VERTEX_SIZE;
                        }
                    }
                    saw_draw = true;
                }
            }
        }

        if !saw_draw && !preserve_contents {
            if current_target_tex_id == 0 {
                clear_rgba_buffer(screen.as_mut_slice(), clear_rgb);
            } else if let Some(Some(target)) =
                textures.get_mut(current_target_tex_id.saturating_sub(1) as usize)
            {
                clear_rgba_buffer(target.rgba.as_mut_slice(), clear_rgb);
            }
        }

        let _ = crate::gfx::publish_screenshot_rgba_buffer(screen_w, screen_h, screen.as_slice());
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
            0x0307 => BlendFactor::OneMinusDstColor, // GL_ONE_MINUS_DST_COLOR
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
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
        let en = enabled != 0;
        let mut st = GFX_CABI_STATE.lock();
        st.cur_blend = BlendDesc {
            enabled: en,
            src: gl_blend_factor_to_core(src_rgb),
            dst: gl_blend_factor_to_core(dst_rgb),
        };
        let frame_seq = st.frame_seq;
        drop(st);
        gfx_trace_record(
            GFX_TRACE_OP_SET_BLEND,
            frame_seq,
            enabled,
            src_rgb,
            dst_rgb,
            _src_alpha,
            _dst_alpha,
        );
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_set_sampler(
        wrap_s: u32,
        wrap_t: u32,
        min_filter: u32,
        mag_filter: u32,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
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
        let frame_seq = st.frame_seq;
        drop(st);
        gfx_trace_record(
            GFX_TRACE_OP_SET_SAMPLER,
            frame_seq,
            0,
            wrap_s,
            wrap_t,
            min_filter,
            mag_filter,
        );
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_set_scissor(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
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
        if st.frame_active {
            let rect = st.cur_scissor;
            st.frame_draws.push(PendingDraw::SetScissor { rect });
        }
        let frame_seq = st.frame_seq;
        drop(st);
        gfx_trace_record(GFX_TRACE_OP_SET_SCISSOR, frame_seq, 0, x, y, width, height);
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_clear_scissor() -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
        let mut st = GFX_CABI_STATE.lock();
        st.cur_scissor = None;
        if st.frame_active {
            st.frame_draws.push(PendingDraw::SetScissor { rect: None });
        }
        let frame_seq = st.frame_seq;
        drop(st);
        gfx_trace_record(GFX_TRACE_OP_CLEAR_SCISSOR, frame_seq, 0, 0, 0, 0, 0);
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_clear_rect_no_present(
        rgb: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
        let mut st = GFX_CABI_STATE.lock();
        if !st.frame_active {
            return -1;
        }
        if width == 0 || height == 0 {
            return 0;
        }
        st.frame_draws.push(PendingDraw::ClearRect {
            rgb,
            x,
            y,
            width,
            height,
        });
        let frame_seq = st.frame_seq;
        drop(st);
        gfx_trace_record(
            GFX_TRACE_OP_CLEAR_RECT,
            frame_seq,
            rgb & 0x00FF_FFFF,
            x,
            y,
            width,
            height,
        );
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_set_render_target(tex_id: u32) -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
        let mut st = GFX_CABI_STATE.lock();
        if tex_id == 0 {
            let had_scissor = st.cur_scissor.take().is_some();
            st.frame_render_target_tex_id = 0;
            st.viewport_configured = false;
            if st.frame_active {
                if had_scissor {
                    st.frame_draws.push(PendingDraw::SetScissor { rect: None });
                }
                st.frame_draws
                    .push(PendingDraw::SetRenderTarget { tex_id: 0 });
            }
            let frame_seq = st.frame_seq;
            drop(st);
            gfx_trace_record(GFX_TRACE_OP_SET_RENDER_TARGET, frame_seq, 0, 0, 0, 0, 0);
            return 0;
        }
        let idx = tex_id.saturating_sub(1) as usize;
        let entry = st
            .tex_images
            .as_mut()
            .and_then(|images| images.get_mut(idx))
            .and_then(|entry| entry.as_mut());
        let Some(entry) = entry else {
            return -1;
        };
        entry.origin = TexCoordOrigin::BottomLeft;
        let had_scissor = st.cur_scissor.take().is_some();
        st.frame_render_target_tex_id = tex_id;
        st.viewport_configured = false;
        if st.frame_active {
            if had_scissor {
                st.frame_draws.push(PendingDraw::SetScissor { rect: None });
            }
            st.frame_draws.push(PendingDraw::SetRenderTarget { tex_id });
        }
        let frame_seq = st.frame_seq;
        drop(st);
        gfx_trace_record(GFX_TRACE_OP_SET_RENDER_TARGET, frame_seq, 0, tex_id, 0, 0, 0);
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_clear_render_target() -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
        let mut st = GFX_CABI_STATE.lock();
        let had_scissor = st.cur_scissor.take().is_some();
        st.frame_render_target_tex_id = 0;
        st.viewport_configured = false;
        if st.frame_active {
            if had_scissor {
                st.frame_draws.push(PendingDraw::SetScissor { rect: None });
            }
            st.frame_draws
                .push(PendingDraw::SetRenderTarget { tex_id: 0 });
        }
        let frame_seq = st.frame_seq;
        drop(st);
        gfx_trace_record(GFX_TRACE_OP_CLEAR_RENDER_TARGET, frame_seq, 0, 0, 0, 0, 0);
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
        let n = crate::logflag::GFX_CABI_SUBMIT_BUDGET_LOGS
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
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
            st.tex_pipeline_mask = PipelineId::invalid();
            st.tex_pipeline_rgba = PipelineId::invalid();
            st.tex_pipeline_particle = PipelineId::invalid();
            st.tex_pipeline_mandelbrot = PipelineId::invalid();
            st.tex_vbuf = [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN];
            st.tex_capacity = [0; GFX_CABI_VBUF_RING_LEN];
            st.tex_images = None;
            st.swapchain_configured = false;
            st.viewport_configured = false;
            st.frame_active = false;
            st.frame_allow_screen_present = true;
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
            st.base_cache_screen_width = 0;
            st.base_cache_screen_height = 0;
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
        pipeline_kind: TexPipelineKind,
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
            st.tex_pipeline_mask = PipelineId::invalid();
            st.tex_pipeline_rgba = PipelineId::invalid();
            st.tex_pipeline_particle = PipelineId::invalid();
            st.tex_pipeline_mandelbrot = PipelineId::invalid();
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
            st.base_cache_screen_width = 0;
            st.base_cache_screen_height = 0;
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

        let mut pipeline_id = match pipeline_kind {
            TexPipelineKind::Mask => st.tex_pipeline_mask,
            TexPipelineKind::Rgba => st.tex_pipeline_rgba,
            TexPipelineKind::Particle => st.tex_pipeline_particle,
            TexPipelineKind::Mandelbrot => st.tex_pipeline_mandelbrot,
        };

        if !pipeline_id.is_valid() {
            let layout = VertexLayout {
                stride: 20, // f32 x,y, f32 u,v, u8 r,g,b,a
                pos_offset: 0,
                color_offset: 16,
                color_format: ColorFormat::RgbaU8,
                texcoord_offset: 8,
                texcoord_format: TexCoordFormat::UvF32,
            };
            let fs_tag = match pipeline_kind {
                TexPipelineKind::Mask => ShaderId::from_raw(TEX_PIPELINE_FS_MASK_TAG_RAW),
                TexPipelineKind::Rgba => ShaderId::from_raw(TEX_PIPELINE_FS_RGBA_TAG_RAW),
                TexPipelineKind::Particle => ShaderId::from_raw(TEX_PIPELINE_FS_PARTICLE_TAG_RAW),
                TexPipelineKind::Mandelbrot => {
                    ShaderId::from_raw(crate::gfx::mandelbrot::MANDELBROT_PIPELINE_FS_TAG_RAW)
                }
            };
            let p = ctx
                .create_pipeline(PipelineDesc {
                    vertex_layout: layout,
                    vs: None,
                    fs: Some(fs_tag),
                })
                .ok()?;
            pipeline_id = p;
            match pipeline_kind {
                TexPipelineKind::Mask => st.tex_pipeline_mask = p,
                TexPipelineKind::Rgba => st.tex_pipeline_rgba = p,
                TexPipelineKind::Particle => st.tex_pipeline_particle = p,
                TexPipelineKind::Mandelbrot => st.tex_pipeline_mandelbrot = p,
            }
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
        Some((pipeline_id, st.tex_vbuf[idx], need_set_viewport))
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
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
        crate::gfx::init(None);

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

        let Some(ret) =
            crate::gfx::with_context_tag(crate::gfx::SystemLockOwner::DrawRgbTriangles, |ctx| {
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
            })
        else {
            return -6;
        };
        ret
    }

    fn render_rgb_triangles_to_texture_now(tex_id: u32, clear_rgb: u32, vtx: &[u8]) -> i32 {
        crate::gfx::init(None);

        if tex_id == 0 {
            return -1;
        }
        if reject_unreasonable_tex_id(tex_id, "render-rgb-now") {
            return -7;
        }
        if vtx.is_empty() {
            return 0;
        }
        const VTX_SIZE: usize = 12;
        let usable = vtx.len() - (vtx.len() % VTX_SIZE);
        if usable == 0 {
            return -2;
        }
        let vcount = (usable / VTX_SIZE) as u32;
        if vcount == 0 {
            return 0;
        }

        let (image, width, height) = {
            let st = GFX_CABI_STATE.lock();
            let idx = tex_id.saturating_sub(1) as usize;
            let Some(entry) = st
                .tex_images
                .as_ref()
                .and_then(|images| images.get(idx))
                .and_then(|entry| entry.as_ref())
            else {
                return -5;
            };
            (entry.image, entry.width.max(1), entry.height.max(1))
        };

        if !image.is_valid() {
            return -6;
        }

        crate::gfx::with_cabi_frame_lock(|| {
            let Some(ret) = crate::gfx::with_context_tag(
                crate::gfx::SystemLockOwner::DrawRgbTriangles,
                |ctx| {
                    let (pipeline, vbuf, _) = match ensure_gfx_resources(ctx, usable) {
                        Some(v) => v,
                        None => return -3,
                    };

                    if ctx.write_buffer(vbuf, 0, &vtx[..usable]).is_err() {
                        return -4;
                    }

                    let cmds = [
                        Command::SetRenderTarget(Some(image)),
                        Command::SetViewport(Viewport {
                            x: 0,
                            y: 0,
                            width: width as i32,
                            height: height as i32,
                        }),
                        Command::SetScissor(None),
                        Command::SetBlend(trueos_gfx_core::BlendDesc::disabled()),
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
                    ];
                    if !check_submit_budget(usable, cmds.len(), "draw_rgb_triangles_to_texture") {
                        return -7;
                    }
                    if ctx.submit(CommandBuffer { commands: &cmds }).is_err() {
                        return -8;
                    }
                    0
                },
            ) else {
                return -9;
            };
            if ret == 0 {
                let mut st = GFX_CABI_STATE.lock();
                if let Some(entry) = st
                    .tex_images
                    .as_mut()
                    .and_then(|images| images.get_mut(tex_id.saturating_sub(1) as usize))
                    .and_then(|entry| entry.as_mut())
                {
                    let Some(need) = checked_reasonable_rgba_len(entry.width, entry.height) else {
                        crate::log!(
                            "gfx-cabi: invalid rgba len for rgb-to-texture tex={} size={}x{}\n",
                            tex_id,
                            entry.width,
                            entry.height
                        );
                        return -10;
                    };
                    if entry.rgba.len() != need {
                        entry.rgba.resize(need, 0);
                    }
                    clear_rgba_buffer(entry.rgba.as_mut_slice(), clear_rgb);
                    let mut off = 0usize;
                    while off + (3 * RGB_VERTEX_SIZE) <= usable {
                        let Some(v0) = read_rgb_vtx(&vtx[..usable], off) else {
                            break;
                        };
                        let Some(v1) = read_rgb_vtx(&vtx[..usable], off + RGB_VERTEX_SIZE) else {
                            break;
                        };
                        let Some(v2) = read_rgb_vtx(&vtx[..usable], off + (2 * RGB_VERTEX_SIZE))
                        else {
                            break;
                        };
                        draw_rgb_triangle_rgba(
                            entry.rgba.as_mut_slice(),
                            entry.width,
                            entry.height,
                            None,
                            trueos_gfx_core::BlendDesc::disabled(),
                            v0,
                            v1,
                            v2,
                        );
                        off += 3 * RGB_VERTEX_SIZE;
                    }
                }
                st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                st.viewport_configured = false;
            }
            ret
        })
    }

    pub fn render_rgb_triangles_to_texture(tex_id: u32, clear_rgb: u32, vtx: &[u8]) -> i32 {
        render_rgb_triangles_to_texture_now(tex_id, clear_rgb, vtx)
    }

    fn render_mandelbrot_to_texture_now(tex_id: u32, ticks: u64, tick_hz: u64) -> i32 {
        crate::gfx::init(None);

        if tex_id == 0 {
            return -1;
        }
        if reject_unreasonable_tex_id(tex_id, "render-mandelbrot-now") {
            return -8;
        }
        if !crate::gfx::is_virgl_active() {
            return -2;
        }

        let verts = crate::gfx::mandelbrot::fullscreen_quad_rgba_bytes_for_view(ticks, tick_hz);
        let usable = verts.len();
        if usable == 0 {
            return -3;
        }

        let (image, width, height) = {
            let st = GFX_CABI_STATE.lock();
            let idx = tex_id.saturating_sub(1) as usize;
            let Some(entry) = st
                .tex_images
                .as_ref()
                .and_then(|images| images.get(idx))
                .and_then(|entry| entry.as_ref())
            else {
                return -6;
            };
            (entry.image, entry.width.max(1), entry.height.max(1))
        };

        if !image.is_valid() {
            return -7;
        }

        crate::gfx::with_cabi_frame_lock(|| {
            let Some(ret) =
                crate::gfx::with_context_tag(crate::gfx::SystemLockOwner::DrawMandelbrot, |ctx| {
                    let (pipeline, vbuf, _) =
                        match ensure_gfx_resources_tex(ctx, usable, TexPipelineKind::Mandelbrot) {
                            Some(v) => v,
                            None => return -4,
                        };

                    if ctx.write_buffer(vbuf, 0, verts.as_slice()).is_err() {
                        return -5;
                    }

                    let cmds = [
                        Command::SetRenderTarget(Some(image)),
                        Command::SetViewport(Viewport {
                            x: 0,
                            y: 0,
                            width: width as i32,
                            height: height as i32,
                        }),
                        Command::SetScissor(None),
                        Command::SetBlend(trueos_gfx_core::BlendDesc::disabled()),
                        Command::BindPipeline(pipeline),
                        Command::BindVertexBuffer {
                            buffer: vbuf,
                            offset: 0,
                        },
                        Command::Draw {
                            vertex_count: 6,
                            first_vertex: 0,
                        },
                    ];
                    if !check_submit_budget(usable, cmds.len(), "draw_mandelbrot_to_texture") {
                        return -8;
                    }
                    if ctx.submit(CommandBuffer { commands: &cmds }).is_err() {
                        return -9;
                    }
                    0
                })
            else {
                return -10;
            };
            if ret == 0 {
                let mut st = GFX_CABI_STATE.lock();
                st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                st.viewport_configured = false;
            }
            ret
        })
    }

    fn render_tex_triangles_to_texture_now(
        target_tex_id: u32,
        source_tex_id: u32,
        clear_rgb: u32,
        vtx: &[u8],
        particle_shader: bool,
    ) -> i32 {
        crate::gfx::init(None);

        if target_tex_id == 0 || source_tex_id == 0 {
            return -1;
        }
        if reject_unreasonable_tex_pair(target_tex_id, source_tex_id, "render-tex-now") {
            return -8;
        }
        const VTX_SIZE: usize = 20;
        let usable = vtx.len() - (vtx.len() % VTX_SIZE);
        if usable == 0 {
            return -2;
        }
        let vcount = (usable / VTX_SIZE) as u32;
        if vcount == 0 {
            return 0;
        }

        let (target_image, target_width, target_height) = {
            let st = GFX_CABI_STATE.lock();
            let idx = target_tex_id.saturating_sub(1) as usize;
            let Some(entry) = st
                .tex_images
                .as_ref()
                .and_then(|images| images.get(idx))
                .and_then(|entry| entry.as_ref())
            else {
                return -3;
            };
            (entry.image, entry.width.max(1), entry.height.max(1))
        };

        let (source_image, pipeline_kind, source_sample_kind) = {
            let st = GFX_CABI_STATE.lock();
            let idx = source_tex_id.saturating_sub(1) as usize;
            let Some(entry) = st
                .tex_images
                .as_ref()
                .and_then(|images| images.get(idx))
                .and_then(|entry| entry.as_ref())
            else {
                return -4;
            };
            let kind = if particle_shader {
                TexPipelineKind::Particle
            } else {
                match entry.sample_kind {
                    TexSampleKind::Mask => TexPipelineKind::Mask,
                    TexSampleKind::Rgba => TexPipelineKind::Rgba,
                }
            };
            (
                entry.image,
                kind,
                if particle_shader {
                    TexSampleKind::Rgba
                } else {
                    entry.sample_kind
                },
            )
        };

        if !target_image.is_valid() || !source_image.is_valid() {
            return -5;
        }

        crate::gfx::with_cabi_frame_lock(|| {
            let Some(ret) =
                crate::gfx::with_context_tag(crate::gfx::SystemLockOwner::UploadTexture, |ctx| {
                    let (pipeline, vbuf, _) =
                        match ensure_gfx_resources_tex(ctx, usable, pipeline_kind) {
                            Some(v) => v,
                            None => return -6,
                        };

                    if ctx.write_buffer(vbuf, 0, &vtx[..usable]).is_err() {
                        return -7;
                    }

                    let cmds = [
                        Command::SetRenderTarget(Some(target_image)),
                        Command::SetViewport(Viewport {
                            x: 0,
                            y: 0,
                            width: target_width as i32,
                            height: target_height as i32,
                        }),
                        Command::SetScissor(None),
                        Command::SetBlend(trueos_gfx_core::BlendDesc::straight_alpha()),
                        Command::ClearColor { rgb: clear_rgb },
                        Command::BindPipeline(pipeline),
                        Command::SetSampler(SamplerDesc {
                            wrap_s: SamplerWrap::ClampToEdge,
                            wrap_t: SamplerWrap::ClampToEdge,
                            min_filter: SamplerFilter::Nearest,
                            mag_filter: SamplerFilter::Nearest,
                        }),
                        Command::BindImage(source_image),
                        Command::BindVertexBuffer {
                            buffer: vbuf,
                            offset: 0,
                        },
                        Command::Draw {
                            vertex_count: vcount,
                            first_vertex: 0,
                        },
                    ];
                    if !check_submit_budget(usable, cmds.len(), "draw_tex_triangles_to_texture") {
                        return -8;
                    }
                    if ctx.submit(CommandBuffer { commands: &cmds }).is_err() {
                        return -9;
                    }
                    0
                })
            else {
                return -10;
            };
            if ret == 0 {
                let mut st = GFX_CABI_STATE.lock();
                let target_idx = target_tex_id.saturating_sub(1) as usize;
                let source_idx = source_tex_id.saturating_sub(1) as usize;
                let Some(images) = st.tex_images.as_mut() else {
                    return ret;
                };
                if target_idx >= images.len() || source_idx >= images.len() {
                    return ret;
                }
                if target_idx == source_idx {
                    crate::log!(
                        "gfx-cabi: tex-to-texture mirror skipped target==source tex={} src={}\n",
                        target_tex_id,
                        source_tex_id
                    );
                    return ret;
                }
                let (source_tex, target) = if source_idx < target_idx {
                    let (left, right) = images.split_at_mut(target_idx);
                    let Some(source_tex) = left.get(source_idx).and_then(|entry| entry.as_ref())
                    else {
                        return ret;
                    };
                    let Some(target) = right.get_mut(0).and_then(|entry| entry.as_mut()) else {
                        return ret;
                    };
                    (source_tex, target)
                } else {
                    let (left, right) = images.split_at_mut(source_idx);
                    let Some(target) = left.get_mut(target_idx).and_then(|entry| entry.as_mut())
                    else {
                        return ret;
                    };
                    let Some(source_tex) = right.get(0).and_then(|entry| entry.as_ref()) else {
                        return ret;
                    };
                    (source_tex, target)
                };
                let Some(need) = checked_reasonable_rgba_len(target.width, target.height) else {
                    crate::log!(
                        "gfx-cabi: invalid rgba len for tex-to-texture tex={} size={}x{} src={}\n",
                        target_tex_id,
                        target.width,
                        target.height,
                        source_tex_id
                    );
                    return -11;
                };
                if target.rgba.len() != need {
                    target.rgba.resize(need, 0);
                }
                clear_rgba_buffer(target.rgba.as_mut_slice(), clear_rgb);
                let mut off = 0usize;
                while off + (3 * TEX_VERTEX_SIZE) <= usable {
                    let Some(v0) = read_tex_vtx(&vtx[..usable], off) else {
                        break;
                    };
                    let Some(v1) = read_tex_vtx(&vtx[..usable], off + TEX_VERTEX_SIZE) else {
                        break;
                    };
                    let Some(v2) = read_tex_vtx(&vtx[..usable], off + (2 * TEX_VERTEX_SIZE)) else {
                        break;
                    };
                    draw_tex_triangle_rgba(
                        target.rgba.as_mut_slice(),
                        target.width,
                        target.height,
                        None,
                        trueos_gfx_core::BlendDesc::straight_alpha(),
                        SamplerDesc {
                            wrap_s: SamplerWrap::ClampToEdge,
                            wrap_t: SamplerWrap::ClampToEdge,
                            min_filter: SamplerFilter::Nearest,
                            mag_filter: SamplerFilter::Nearest,
                        },
                        source_sample_kind,
                        source_tex,
                        v0,
                        v1,
                        v2,
                    );
                    off += 3 * TEX_VERTEX_SIZE;
                }
                st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                st.viewport_configured = false;
            }
            ret
        })
    }

    fn upload_texture_rgba_inner_impl(
        tex_id: u32,
        width: u32,
        height: u32,
        region: Option<ImageRegion>,
        data_ptr: *const u8,
        data_len: usize,
        mut owned_rgba: Option<Vec<u8>>,
        sample_kind: TexSampleKind,
        call_init: bool,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            if crate::hv::current_hull_guest_context_vm_id().is_some() {
                if let Some(rgba) = owned_rgba.as_ref() {
                    return vmcall_texture_rgba_upload_from_ptr(
                        tex_id,
                        width,
                        height,
                        region,
                        rgba.as_ptr(),
                        rgba.len(),
                        sample_kind,
                    );
                }
                return vmcall_texture_rgba_upload_from_ptr(
                    tex_id,
                    width,
                    height,
                    region,
                    data_ptr,
                    data_len,
                    sample_kind,
                );
            }
            let reason = if call_init {
                "vm-upload-rgba"
            } else {
                "vm-upload-rgba-no-init"
            };
            if let Some(rgba) = owned_rgba.take() {
                if queue_texture_rgba_upload_owned(
                    tex_id,
                    width,
                    height,
                    region,
                    rgba,
                    sample_kind,
                    0,
                    reason,
                    false,
                ) {
                    return 0;
                }
                return -4;
            }
            return queue_texture_rgba_upload_from_ptr(
                tex_id,
                width,
                height,
                region,
                data_ptr,
                data_len,
                sample_kind,
                reason,
            );
        }
        if call_init {
            crate::gfx::init(None);
        }
        gfx_trace_record(
            GFX_TRACE_OP_UPLOAD_TEXTURE_RGBA,
            0,
            match sample_kind {
                TexSampleKind::Mask => 0,
                TexSampleKind::Rgba => 1,
            },
            tex_id,
            width,
            height,
            data_len.min(u32::MAX as usize) as u32,
        );

        if tex_id == 0 || width == 0 || height == 0 {
            return -1;
        }
        if reject_unreasonable_tex_id(tex_id, "upload-rgba-now") {
            return -8;
        }
        if data_ptr.is_null() {
            return -2;
        }
        let expected = match region {
            Some(region) => (region.width as usize)
                .saturating_mul(region.height as usize)
                .saturating_mul(4),
            None => (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(4),
        };
        if data_len < expected {
            return -3;
        }
        let data = unsafe { core::slice::from_raw_parts(data_ptr, expected) };

        let Some(ret) = crate::gfx::with_context_tag(
            crate::gfx::SystemLockOwner::UploadTexture,
            |ctx| {
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
                    st.tex_pipeline_mask = PipelineId::invalid();
                    st.tex_pipeline_rgba = PipelineId::invalid();
                    st.tex_pipeline_particle = PipelineId::invalid();
                    st.tex_pipeline_mandelbrot = PipelineId::invalid();
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
                    st.base_cache_screen_width = 0;
                    st.base_cache_screen_height = 0;
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
                        crate::log!(
                            "gfx-cabi: create_image failed for upload tex={} size={}x{} region={} kind={}\n",
                            tex_id,
                            width,
                            height,
                            region.is_some() as u8,
                            match sample_kind {
                                TexSampleKind::Mask => "mask",
                                TexSampleKind::Rgba => "rgba",
                            }
                        );
                        return -4;
                    };
                    image_id = img;
                }
                let used_owned_full_rgba = region.is_none() && owned_rgba.is_some();
                let Some(full_rgba_len) = checked_reasonable_rgba_len(width, height) else {
                    crate::log!(
                        "gfx-cabi: invalid rgba len for upload tex={} size={}x{} region={}\n",
                        tex_id,
                        width,
                        height,
                        region.is_some() as u8
                    );
                    return -7;
                };
                let mut cached_rgba = if region.is_none() {
                    if let Some(mut rgba) = owned_rgba.take() {
                        rgba.truncate(expected);
                        rgba
                    } else if recreate {
                        vec![0; full_rgba_len]
                    } else {
                        images[idx]
                            .as_ref()
                            .map(|entry| entry.rgba.clone())
                            .unwrap_or_else(|| vec![0; full_rgba_len])
                    }
                } else if recreate {
                    vec![0; full_rgba_len]
                } else {
                    images[idx]
                        .as_ref()
                        .map(|entry| entry.rgba.clone())
                        .unwrap_or_else(|| vec![0; full_rgba_len])
                };
                match region {
                    Some(region) => {
                        for row in 0..region.height as usize {
                            let src_off =
                                row.saturating_mul(region.width as usize).saturating_mul(4);
                            let dst_off = ((region.y as usize + row)
                                .saturating_mul(width as usize)
                                .saturating_add(region.x as usize))
                            .saturating_mul(4);
                            let row_len = region.width as usize * 4;
                            cached_rgba[dst_off..dst_off + row_len]
                                .copy_from_slice(&data[src_off..src_off + row_len]);
                        }
                    }
                    None => {
                        if !used_owned_full_rgba {
                            cached_rgba[..expected].copy_from_slice(&data[..expected]);
                        }
                    }
                }
                images[idx] = Some(TexImage {
                    image: image_id,
                    width,
                    height,
                    sample_kind,
                    origin: TexCoordOrigin::TopLeft,
                    rgba: cached_rgba,
                });
                let write_res = match region {
                    Some(region) if !recreate => ctx.write_image_region(image_id, region, data),
                    _ => ctx.write_image(image_id, images[idx].as_ref().unwrap().rgba.as_slice()),
                };
                if write_res.is_err() {
                    return -5;
                }
                0
            },
        ) else {
            return -6;
        };
        ret
    }

    fn queue_texture_rgba_upload_from_ptr(
        tex_id: u32,
        width: u32,
        height: u32,
        region: Option<ImageRegion>,
        data_ptr: *const u8,
        data_len: usize,
        sample_kind: TexSampleKind,
        repaint_reason: &'static str,
    ) -> i32 {
        if tex_id == 0 || width == 0 || height == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        let expected = match region {
            Some(region) => {
                if region.width == 0
                    || region.height == 0
                    || region.x.saturating_add(region.width) > width
                    || region.y.saturating_add(region.height) > height
                {
                    return -1;
                }
                (region.width as usize)
                    .saturating_mul(region.height as usize)
                    .saturating_mul(4)
            }
            None => (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(4),
        };
        if data_len < expected {
            return -3;
        }
        let data = unsafe { core::slice::from_raw_parts(data_ptr, expected) };
        if queue_texture_rgba_upload_owned(
            tex_id,
            width,
            height,
            region,
            data.to_vec(),
            sample_kind,
            0,
            repaint_reason,
            false,
        ) {
            0
        } else {
            -4
        }
    }

    #[inline]
    fn upload_texture_rgba_inner(
        tex_id: u32,
        width: u32,
        height: u32,
        region: Option<ImageRegion>,
        data_ptr: *const u8,
        data_len: usize,
        sample_kind: TexSampleKind,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            if crate::hv::current_hull_guest_context_vm_id().is_some() {
                return vmcall_texture_rgba_upload_from_ptr(
                    tex_id,
                    width,
                    height,
                    region,
                    data_ptr,
                    data_len,
                    sample_kind,
                );
            }
            return queue_texture_rgba_upload_from_ptr(
                tex_id,
                width,
                height,
                region,
                data_ptr,
                data_len,
                sample_kind,
                "vm-upload-rgba",
            );
        }
        upload_texture_rgba_inner_impl(
            tex_id,
            width,
            height,
            region,
            data_ptr,
            data_len,
            None,
            sample_kind,
            true,
        )
    }

    #[inline]
    fn upload_texture_rgba_inner_owned(
        tex_id: u32,
        width: u32,
        height: u32,
        region: Option<ImageRegion>,
        rgba: Vec<u8>,
        sample_kind: TexSampleKind,
    ) -> i32 {
        upload_texture_rgba_inner_impl(
            tex_id,
            width,
            height,
            region,
            rgba.as_ptr(),
            rgba.len(),
            Some(rgba),
            sample_kind,
            true,
        )
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        upload_texture_rgba_inner(
            tex_id,
            width,
            height,
            None,
            data_ptr,
            data_len,
            TexSampleKind::Mask,
        )
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_rgba_image(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        upload_texture_rgba_inner(
            tex_id,
            width,
            height,
            None,
            data_ptr,
            data_len,
            TexSampleKind::Rgba,
        )
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_rgba_image_async(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        if !gfx_cabi_vm_context() {
            crate::gfx::init(None);
        }
        gfx_trace_record(
            GFX_TRACE_OP_UPLOAD_TEXTURE_RGBA,
            0,
            0x8000_0001,
            tex_id,
            width,
            height,
            data_len.min(u32::MAX as usize) as u32,
        );

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
        let data = unsafe { core::slice::from_raw_parts(data_ptr, expected) };
        if !queue_texture_rgba_upload_owned(
            tex_id,
            width,
            height,
            None,
            data.to_vec(),
            TexSampleKind::Rgba,
            0,
            "rgba-async",
            false,
        ) {
            return -4;
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_png(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        if !gfx_cabi_vm_context() {
            crate::gfx::init(None);
        }
        gfx_trace_record(
            GFX_TRACE_OP_UPLOAD_TEXTURE_PNG,
            0,
            0,
            tex_id,
            data_len.min(u32::MAX as usize) as u32,
            0,
            0,
        );

        if tex_id == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        let data = core::slice::from_raw_parts(data_ptr, data_len);
        let decoded = match crate::gfx::png_codec::decode_png_rgba(data) {
            Ok(decoded) => decoded,
            Err(err) => return err.code(),
        };

        upload_texture_rgba_inner(
            tex_id,
            decoded.width,
            decoded.height,
            None,
            decoded.rgba.as_ptr(),
            decoded.rgba.len(),
            TexSampleKind::Rgba,
        )
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_png_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            return unsafe { trueos_cabi_gfx_upload_texture_png(tex_id, data_ptr, data_len) };
        }
        crate::gfx::init(None);
        gfx_trace_record(
            GFX_TRACE_OP_UPLOAD_TEXTURE_PNG,
            0,
            0x8000_0000,
            tex_id,
            data_len.min(u32::MAX as usize) as u32,
            0,
            0,
        );

        if tex_id == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        if data_len == 0 {
            return -3;
        }
        let bytes = unsafe { core::slice::from_raw_parts(data_ptr, data_len) }.to_vec();
        spawn_async_png_decode_upload(tex_id, bytes)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_jpeg(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        if !gfx_cabi_vm_context() {
            crate::gfx::init(None);
        }
        gfx_trace_record(
            GFX_TRACE_OP_UPLOAD_TEXTURE_JPEG,
            0,
            0,
            tex_id,
            data_len.min(u32::MAX as usize) as u32,
            0,
            0,
        );

        if tex_id == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        let data = core::slice::from_raw_parts(data_ptr, data_len);
        let decoded = match crate::gfx::jpeg_codec::decode_jpeg_rgba(data) {
            Ok(decoded) => decoded,
            Err(err) => return err.code(),
        };

        upload_texture_rgba_inner(
            tex_id,
            decoded.width,
            decoded.height,
            None,
            decoded.rgba.as_ptr(),
            decoded.rgba.len(),
            TexSampleKind::Rgba,
        )
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_jpeg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            return unsafe { trueos_cabi_gfx_upload_texture_jpeg(tex_id, data_ptr, data_len) };
        }
        crate::gfx::init(None);
        gfx_trace_record(
            GFX_TRACE_OP_UPLOAD_TEXTURE_JPEG,
            0,
            0x8000_0000,
            tex_id,
            data_len.min(u32::MAX as usize) as u32,
            0,
            0,
        );

        if tex_id == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        if data_len == 0 {
            return -3;
        }
        let bytes = unsafe { core::slice::from_raw_parts(data_ptr, data_len) }.to_vec();
        set_async_tex_status(tex_id, ASYNC_TEX_STATUS_PENDING);
        enqueue_async_jpeg_upload(tex_id, bytes);
        try_spawn_async_jpeg_decode_uploads();
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_svg(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        if !gfx_cabi_vm_context() {
            crate::gfx::init(None);
        }
        gfx_trace_record(
            GFX_TRACE_OP_UPLOAD_TEXTURE_SVG,
            0,
            0,
            tex_id,
            data_len.min(u32::MAX as usize) as u32,
            0,
            0,
        );

        if tex_id == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        let data = core::slice::from_raw_parts(data_ptr, data_len);
        if gfx_cabi_vm_context() {
            return match crate::gfx::svg::rasterize_svg_bytes_rgba(data) {
                Ok((info, rgba)) => {
                    if crate::hv::current_hull_guest_context_vm_id().is_some() {
                        return vmcall_texture_rgba_upload_from_ptr(
                            tex_id,
                            info.width,
                            info.height,
                            None,
                            rgba.as_ptr(),
                            rgba.len(),
                            TexSampleKind::Rgba,
                        );
                    }
                    if queue_texture_rgba_upload_owned(
                        tex_id,
                        info.width,
                        info.height,
                        None,
                        rgba,
                        TexSampleKind::Rgba,
                        0,
                        "vm-upload-svg",
                        false,
                    ) {
                        0
                    } else {
                        -5
                    }
                }
                Err(code) => {
                    log_svg_upload_failure("vm-sync-svg", tex_id, data_len, code, Some(data));
                    code
                }
            };
        }
        match crate::gfx::svg::upload_svg_bytes_to_texture(tex_id, data) {
            Ok(_) => 0,
            Err(code) => {
                log_svg_upload_failure("sync-svg", tex_id, data_len, code, Some(data));
                code
            }
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_svg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            return unsafe { trueos_cabi_gfx_upload_texture_svg(tex_id, data_ptr, data_len) };
        }
        crate::gfx::init(None);
        gfx_trace_record(
            GFX_TRACE_OP_UPLOAD_TEXTURE_SVG,
            0,
            0x8000_0000,
            tex_id,
            data_len.min(u32::MAX as usize) as u32,
            0,
            0,
        );

        if tex_id == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        if data_len == 0 {
            return -3;
        }
        let bytes = unsafe { core::slice::from_raw_parts(data_ptr, data_len) }.to_vec();
        set_async_tex_status(tex_id, ASYNC_TEX_STATUS_PENDING);
        enqueue_async_svg_upload(tex_id, bytes);
        try_start_async_svg_worker();
        0
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32 {
        if reject_unreasonable_tex_id(tex_id, "texture-status") {
            return ASYNC_TEX_STATUS_UNKNOWN;
        }
        if gfx_cabi_vm_context() {
            return if vm_texture_dimensions(tex_id).is_some() {
                ASYNC_TEX_STATUS_READY
            } else {
                ASYNC_TEX_STATUS_UNKNOWN
            };
        }
        if get_async_tex_status(tex_id) == ASYNC_TEX_STATUS_PENDING {
            try_start_async_svg_worker();
        }
        let status = get_async_tex_status(tex_id);
        if status != ASYNC_TEX_STATUS_UNKNOWN {
            return status;
        }
        if texture_dimensions_inner(tex_id).is_some() {
            ASYNC_TEX_STATUS_READY
        } else {
            ASYNC_TEX_STATUS_UNKNOWN
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_texture_dimensions(
        tex_id: u32,
        out_width: *mut u32,
        out_height: *mut u32,
    ) -> i32 {
        if out_width.is_null() || out_height.is_null() {
            return -1;
        }
        if gfx_cabi_vm_context() {
            let Some((width, height)) = vm_texture_dimensions(tex_id) else {
                return -2;
            };
            unsafe {
                core::ptr::write(out_width, width);
                core::ptr::write(out_height, height);
            }
            return 0;
        }
        let Some((width, height)) = texture_dimensions_inner(tex_id) else {
            return -2;
        };
        unsafe {
            core::ptr::write(out_width, width);
            core::ptr::write(out_height, height);
        }
        0
    }

    #[inline]
    fn begin_frame_inner(
        clear_rgb: u32,
        preserve_contents: bool,
        allow_screen_present: bool,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
        crate::gfx::init(None);

        let mut st = GFX_CABI_STATE.lock();
        if st.frame_active {
            return -2;
        }
        // Keep CABI epoch aligned at frame start so first-use texture upload does not
        // treat initial bootstrap as a backend switch and invalidate this frame.
        st.epoch = crate::gfx::backend_epoch();
        st.frame_seq = st.frame_seq.wrapping_add(1);
        st.frame_active = true;
        st.frame_allow_screen_present = allow_screen_present;
        st.frame_preserve_contents = preserve_contents;
        st.frame_clear_rgb = clear_rgb;
        st.frame_rgb_draws = 0;
        st.frame_tex_draws = 0;
        st.frame_draw_bytes = 0;
        st.frame_draws.clear();
        st.frame_rgb_blob.clear();
        st.frame_tex_blob.clear();
        st.cur_scissor = None;
        st.frame_render_target_tex_id = 0;
        st.viewport_configured = false;
        let seq = st.frame_seq;
        if crate::logflag::GFX_CABI_FRAME_DEBUG_LOGS && (seq <= 10 || seq.is_multiple_of(20)) {
            crate::globalog::log(format_args!(
                "gfx-cabi: begin seq={} clear=0x{:06X}\n",
                seq,
                clear_rgb & 0x00FF_FFFF
            ));
        }
        let mut flags = 0u32;
        if preserve_contents {
            flags |= 1;
        }
        if allow_screen_present {
            flags |= 2;
        }
        gfx_trace_record(GFX_TRACE_OP_BEGIN_FRAME, seq, flags, clear_rgb & 0x00FF_FFFF, 0, 0, 0);
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32 {
        begin_frame_inner(clear_rgb, false, true)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_begin_frame_preserve(clear_rgb: u32) -> i32 {
        begin_frame_inner(clear_rgb, true, true)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_begin_frame_no_present(clear_rgb: u32) -> i32 {
        begin_frame_inner(clear_rgb, false, false)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_draw_rgb_triangles_no_present(
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
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

        st.frame_rgb_draws = st.frame_rgb_draws.saturating_add(1);
        st.frame_draw_bytes = st.frame_draw_bytes.saturating_add(bytes.len());
        let blend = st.cur_blend;
        let frame_seq = st.frame_seq;
        let mut off = 0usize;
        while off < bytes.len() {
            let rem = bytes.len() - off;
            let chunk = core::cmp::min(MAX_CMDSTREAM_DRAW_BYTES, rem);
            let chunk = chunk - (chunk % VTX_SIZE);
            if chunk == 0 {
                break;
            }
            let blob_offset = st.frame_rgb_blob.len();
            st.frame_rgb_blob
                .extend_from_slice(&bytes[off..off + chunk]);
            st.frame_draws.push(PendingDraw::Rgb {
                blob_offset,
                blob_len: chunk,
                blend,
            });
            off += chunk;
        }
        drop(st);
        gfx_trace_record(
            GFX_TRACE_OP_DRAW_RGB_TRIANGLES,
            frame_seq,
            vcount,
            usable.min(u32::MAX as usize) as u32,
            0,
            0,
            0,
        );
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_queue_render_rgb_triangles_to_texture(
        tex_id: u32,
        clear_rgb: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
        repaint_window_id: u32,
    ) -> i32 {
        if vtx_ptr.is_null() {
            return if vtx_len == 0 { 0 } else { -1 };
        }
        let bytes = core::slice::from_raw_parts(vtx_ptr, vtx_len);
        if queue_render_rgb_triangles_to_texture_copy(
            tex_id,
            clear_rgb,
            bytes,
            repaint_window_id,
            "portal-rgb-triangles",
        ) {
            0
        } else {
            -2
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_queue_render_tex_triangles_to_texture(
        target_tex_id: u32,
        source_tex_id: u32,
        clear_rgb: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
        repaint_window_id: u32,
    ) -> i32 {
        if vtx_ptr.is_null() {
            return if vtx_len == 0 { 0 } else { -1 };
        }
        let bytes = core::slice::from_raw_parts(vtx_ptr, vtx_len);
        if queue_render_tex_triangles_to_texture_copy(
            target_tex_id,
            source_tex_id,
            clear_rgb,
            bytes,
            repaint_window_id,
            "portal-tex-triangles",
        ) {
            0
        } else {
            -2
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_gfx_queue_render_mandelbrot_to_texture(
        tex_id: u32,
        ticks: u64,
        tick_hz: u64,
        repaint_window_id: u32,
    ) -> i32 {
        if queue_render_mandelbrot_to_texture(
            tex_id,
            ticks,
            tick_hz,
            repaint_window_id,
            "portal-mandelbrot",
        ) {
            0
        } else {
            -1
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_draw_tex_triangles_no_present(
        tex_id: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
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
        let (image, sample_kind, origin) = st
            .tex_images
            .as_ref()
            .and_then(|images| images.get(idx))
            .and_then(|e| e.as_ref())
            .map(|e| (e.image, e.sample_kind, e.origin))
            .unwrap_or((ImageId::invalid(), TexSampleKind::Mask, TexCoordOrigin::TopLeft));
        let sampler = st.cur_sampler;
        let blend = st.cur_blend;
        let frame_seq = st.frame_seq;
        let mut off = 0usize;
        while off < usable {
            let rem = usable - off;
            let chunk = core::cmp::min(MAX_CMDSTREAM_DRAW_BYTES, rem);
            let chunk = chunk - (chunk % VTX_SIZE);
            if chunk == 0 {
                break;
            }
            let blob_offset = st.frame_tex_blob.len();
            append_tex_vertices_with_origin(
                &mut st.frame_tex_blob,
                &bytes[off..off + chunk],
                origin,
            );
            st.frame_draws.push(PendingDraw::Tex {
                tex_id,
                image,
                sample_kind,
                sampler,
                blob_offset,
                blob_len: chunk,
                blend,
            });
            off += chunk;
        }
        drop(st);
        let sampler_flags = ((sampler.wrap_s as u32) & 0xFF)
            | (((sampler.wrap_t as u32) & 0xFF) << 8)
            | (((sampler.min_filter as u32) & 0xFF) << 16)
            | (((sampler.mag_filter as u32) & 0xFF) << 24);
        gfx_trace_record(
            GFX_TRACE_OP_DRAW_TEX_TRIANGLES,
            frame_seq,
            vcount,
            tex_id,
            usable.min(u32::MAX as usize) as u32,
            sampler_flags,
            match sample_kind {
                TexSampleKind::Mask => 0,
                TexSampleKind::Rgba => 1,
            },
        );
        0
    }

    fn ui2_font_tier_from_cabi(tier: u32) -> Option<crate::r::ui2::Ui2FontTier> {
        match tier {
            0 => Some(crate::r::ui2::Ui2FontTier::Half),
            1 => Some(crate::r::ui2::Ui2FontTier::OneX),
            2 => Some(crate::r::ui2::Ui2FontTier::TwoX),
            3 => Some(crate::r::ui2::Ui2FontTier::Third),
            _ => None,
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_ui2_font_line_height_px(tier: u32) -> u32 {
        let Some(tier) = ui2_font_tier_from_cabi(tier) else {
            return 0;
        };
        u32::from(crate::r::ui2::ui2_font_native_line_height_px(tier))
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_ui2_font_blit_text_rgba(
        dst_ptr: *mut u8,
        dst_len: usize,
        dst_width: u32,
        dst_height: u32,
        tier: u32,
        x: u32,
        y: u32,
        max_width_px: u32,
        text_ptr: *const u8,
        text_len: usize,
        r: u32,
        g: u32,
        b: u32,
        a: u32,
    ) -> usize {
        if dst_ptr.is_null()
            || dst_width == 0
            || dst_height == 0
            || max_width_px == 0
            || text_ptr.is_null()
            || text_len == 0
        {
            return 0;
        }

        let Some(tier) = ui2_font_tier_from_cabi(tier) else {
            return 0;
        };
        let Some(expected_len) = (dst_width as usize)
            .checked_mul(dst_height as usize)
            .and_then(|px| px.checked_mul(4))
        else {
            return 0;
        };
        if dst_len < expected_len {
            return 0;
        }

        let bytes = unsafe { core::slice::from_raw_parts(text_ptr, text_len) };
        let Ok(text) = core::str::from_utf8(bytes) else {
            return 0;
        };
        let Some(atlases) = crate::r::ui2::ui2_font_decode_cpu_atlases(tier.size_case()) else {
            return 0;
        };
        let dst = unsafe { core::slice::from_raw_parts_mut(dst_ptr, expected_len) };
        crate::r::ui2::ui2_font_blit_text_rgba(
            dst,
            dst_width as usize,
            dst_height as usize,
            &atlases,
            tier,
            x as usize,
            y as usize,
            max_width_px as usize,
            text,
            [
                r.min(u8::MAX as u32) as u8,
                g.min(u8::MAX as u32) as u8,
                b.min(u8::MAX as u32) as u8,
                a.min(u8::MAX as u32) as u8,
            ],
        )
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_end_frame() -> i32 {
        if gfx_cabi_vm_context() {
            return GFX_CABI_VM_HOST_ONLY_RC;
        }
        let _host_alloc_domain = crate::allocators::enter_host_alloc_domain_current_cpu();
        crate::gfx::init(None);

        let (
            seq,
            rgb_draws,
            tex_draws,
            draw_bytes,
            was_active,
            allow_screen_present,
            preserve_contents,
            clear_rgb,
            draws,
            rgb_src,
            tex_src,
        ) = {
            let mut st = GFX_CABI_STATE.lock();
            let out = (
                st.frame_seq,
                st.frame_rgb_draws,
                st.frame_tex_draws,
                st.frame_draw_bytes,
                st.frame_active,
                st.frame_allow_screen_present,
                st.frame_preserve_contents,
                st.frame_clear_rgb,
                core::mem::take(&mut st.frame_draws),
                core::mem::take(&mut st.frame_rgb_blob),
                core::mem::take(&mut st.frame_tex_blob),
            );
            st.frame_active = false;
            st.frame_allow_screen_present = true;
            st.frame_preserve_contents = false;
            st.frame_render_target_tex_id = 0;
            out
        };
        if !was_active {
            crate::globalog::log(format_args!("gfx-cabi: end without active frame\n"));
            return -3;
        }
        let mut end_flags = 0u32;
        if allow_screen_present {
            end_flags |= 1;
        }
        if preserve_contents {
            end_flags |= 2;
        }
        gfx_trace_record(
            GFX_TRACE_OP_END_FRAME,
            seq,
            end_flags,
            rgb_draws,
            tex_draws,
            draw_bytes.min(u32::MAX as usize) as u32,
            0,
        );

        let mut final_render_target_tex_id = 0u32;
        for draw in &draws {
            if let PendingDraw::SetRenderTarget { tex_id } = draw {
                final_render_target_tex_id = *tex_id;
            }
        }
        let is_screen_present_frame = final_render_target_tex_id == 0;
        if is_screen_present_frame && !allow_screen_present {
            if rgb_draws > 0 || tex_draws > 0 {
                crate::globalog::log(format_args!(
                    "gfx-cabi: end rejected screen-present in no-present frame seq={} rgb={} tex={} bytes={}\n",
                    seq, rgb_draws, tex_draws, draw_bytes
                ));
            }
            return -4;
        }

        let resolved_tex_images: Vec<Option<(ImageId, u32, u32)>> = {
            let st = GFX_CABI_STATE.lock();
            st.tex_images
                .as_ref()
                .map(|images| {
                    images
                        .iter()
                        .map(|entry| {
                            entry
                                .as_ref()
                                .map(|img| (img.image, img.width.max(1), img.height.max(1)))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut submitted_passes = 0usize;
        let mut screenshot_overlay_extent: Option<(u32, u32)> = None;

        let Some(ret) = ({
            let _end_frame_guard = EndFrameProgressGuard::new();
            crate::gfx::with_context_tag(crate::gfx::SystemLockOwner::EndFrame, |ctx| {
                let (_p, _v, need_set_viewport) = match ensure_gfx_resources(ctx, 0) {
                    Some(v) => v,
                    None => return -1,
                };
                let swap = ctx.swapchain_desc();
                if is_screen_present_frame {
                    screenshot_overlay_extent = Some((swap.extent.width, swap.extent.height));
                }
                // Compose cursor into app-driven presents to avoid one-frame cursor blink
                // between end_frame and the async cursor overlay tick.
                let mut submit_draws = draws.clone();
                let mut submit_rgb_src = rgb_src.clone();
                let mut submit_tex_src = tex_src.clone();
                if is_screen_present_frame {
                    append_kernel_cursor_overlay_draws(
                        &mut submit_draws,
                        &mut submit_rgb_src,
                        &mut submit_tex_src,
                        swap.extent.width,
                        swap.extent.height,
                        None,
                    );
                }
                const MAX_PASS_VERTEX_BYTES: usize = 96 * 1024;

                enum Plan {
                    SetRenderTarget {
                        image: Option<ImageId>,
                        vp_w: u32,
                        vp_h: u32,
                    },
                    SetScissor {
                        rect: Option<ScissorRect>,
                    },
                    ClearRect {
                        rgb: u32,
                        x: u32,
                        y: u32,
                        width: u32,
                        height: u32,
                    },
                    Rgb {
                        offset: u64,
                        vcount: u32,
                        blend: BlendDesc,
                    },
                    Tex {
                        tex_id: u32,
                        image: ImageId,
                        sample_kind: TexSampleKind,
                        sampler: SamplerDesc,
                        offset: u64,
                        vcount: u32,
                        blend: BlendDesc,
                    },
                }

                let mut draw_idx = 0usize;
                let mut first_pass = true;
                let mut current_target_image: Option<ImageId> = None;
                let mut current_vp_w = swap.extent.width;
                let mut current_vp_h = swap.extent.height;

                while draw_idx < submit_draws.len() {
                    let start = draw_idx;
                    let mut pass_bytes = 0usize;
                    let mut pass_kind: u8 = 0; // 1=rgb, 2=tex
                    let mut pass_tex_kind: Option<TexSampleKind> = None;
                    while draw_idx < submit_draws.len() {
                        let (kind, add, tex_kind) = match &submit_draws[draw_idx] {
                            PendingDraw::SetRenderTarget { .. } => {
                                if pass_kind == 0 {
                                    draw_idx += 1;
                                    continue;
                                }
                                break;
                            }
                            PendingDraw::SetScissor { .. } => {
                                if pass_kind == 0 {
                                    draw_idx += 1;
                                    continue;
                                }
                                break;
                            }
                            PendingDraw::ClearRect { .. } => {
                                if pass_kind == 0 {
                                    draw_idx += 1;
                                    continue;
                                }
                                break;
                            }
                            PendingDraw::Rgb { blob_len, .. } => {
                                (1u8, blob_len - (blob_len % 12), None)
                            }
                            PendingDraw::Tex {
                                blob_len,
                                sample_kind,
                                ..
                            } => (2u8, blob_len - (blob_len % 20), Some(*sample_kind)),
                        };
                        if add == 0 {
                            draw_idx += 1;
                            continue;
                        }
                        if pass_kind == 0 {
                            pass_kind = kind;
                            pass_tex_kind = tex_kind;
                        } else if kind != pass_kind {
                            // Keep pass submissions homogeneous by vertex format/pipeline type.
                            break;
                        } else if kind == 2 && tex_kind != pass_tex_kind {
                            break;
                        }
                        if pass_bytes != 0 && pass_bytes.saturating_add(add) > MAX_PASS_VERTEX_BYTES
                        {
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
                            PendingDraw::SetRenderTarget { tex_id } => {
                                if *tex_id == 0 {
                                    current_target_image = None;
                                    current_vp_w = swap.extent.width;
                                    current_vp_h = swap.extent.height;
                                } else {
                                    let idx = tex_id.saturating_sub(1) as usize;
                                    let Some((image, width, height)) =
                                        resolved_tex_images.get(idx).and_then(|entry| *entry)
                                    else {
                                        return -12;
                                    };
                                    current_target_image = Some(image);
                                    current_vp_w = width;
                                    current_vp_h = height;
                                }
                                plans.push(Plan::SetRenderTarget {
                                    image: current_target_image,
                                    vp_w: current_vp_w,
                                    vp_h: current_vp_h,
                                });
                            }
                            PendingDraw::SetScissor { rect } => {
                                plans.push(Plan::SetScissor { rect: *rect });
                            }
                            PendingDraw::ClearRect {
                                rgb,
                                x,
                                y,
                                width,
                                height,
                            } => {
                                plans.push(Plan::ClearRect {
                                    rgb: *rgb,
                                    x: *x,
                                    y: *y,
                                    width: *width,
                                    height: *height,
                                });
                            }
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
                                sample_kind,
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
                                    sample_kind: *sample_kind,
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
                        let tex_kind = if plans
                            .iter()
                            .filter_map(|plan| match plan {
                                Plan::Tex { sample_kind, .. } => Some(*sample_kind),
                                _ => None,
                            })
                            .all(|sample_kind| sample_kind == TexSampleKind::Rgba)
                        {
                            TexSampleKind::Rgba
                        } else {
                            TexSampleKind::Mask
                        };
                        let (pipeline, vbuf, _) = match ensure_gfx_resources_tex(
                            ctx,
                            tex_blob.len(),
                            match tex_kind {
                                TexSampleKind::Mask => TexPipelineKind::Mask,
                                TexSampleKind::Rgba => TexPipelineKind::Rgba,
                            },
                        ) {
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
                    let mut pass_target_image = current_target_image;
                    let mut pass_vp_w = current_vp_w;
                    let mut pass_vp_h = current_vp_h;
                    if let Some(Plan::SetRenderTarget { image, vp_w, vp_h }) = plans.first() {
                        pass_target_image = *image;
                        pass_vp_w = *vp_w;
                        pass_vp_h = *vp_h;
                    }
                    if first_pass && need_set_viewport {
                        cmds.push(Command::SetViewport(Viewport {
                            x: 0,
                            y: 0,
                            width: pass_vp_w as i32,
                            height: pass_vp_h as i32,
                        }));
                    }
                    cmds.push(Command::SetRenderTarget(pass_target_image));
                    if first_pass && !preserve_contents {
                        cmds.push(Command::ClearColor { rgb: clear_rgb });
                    }

                    let mut last_blend: Option<BlendDesc> = None;
                    let mut pass_final_target_image = pass_target_image;

                    for plan in plans.iter() {
                        match *plan {
                            Plan::SetRenderTarget { image, vp_w, vp_h } => {
                                pass_final_target_image = image;
                                cmds.push(Command::SetRenderTarget(image));
                                cmds.push(Command::SetViewport(Viewport {
                                    x: 0,
                                    y: 0,
                                    width: vp_w as i32,
                                    height: vp_h as i32,
                                }));
                            }
                            Plan::SetScissor { rect } => {
                                cmds.push(Command::SetScissor(rect.map(|scissor| {
                                    GfxScissorRect {
                                        x: scissor.x,
                                        y: scissor.y,
                                        width: scissor.width,
                                        height: scissor.height,
                                    }
                                })));
                            }
                            Plan::ClearRect {
                                rgb,
                                x,
                                y,
                                width,
                                height,
                            } => {
                                cmds.push(Command::ClearRect {
                                    rgb,
                                    x,
                                    y,
                                    width,
                                    height,
                                });
                            }
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
                                sample_kind: _,
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
                                    let idx = tex_id.saturating_sub(1) as usize;
                                    let Some((resolved_image, _, _)) =
                                        resolved_tex_images.get(idx).and_then(|entry| *entry)
                                    else {
                                        return -10;
                                    };
                                    (resolved_image, false)
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

                    if is_last_pass && pass_final_target_image.is_none() {
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
                        submitted_passes = submitted_passes.saturating_add(1);
                    } else {
                        if let Err(e) = submit_res {
                            crate::globalog::log(format_args!(
                                "gfx-cabi: submit failed: {:?}\n",
                                e
                            ));
                        }
                        return -11;
                    }
                    first_pass = false;
                }

                if first_pass {
                    // No valid draw payloads in this frame; keep clear/present behavior.
                    let mut cmds: Vec<Command> = Vec::new();
                    if need_set_viewport {
                        cmds.push(Command::SetViewport(Viewport {
                            x: 0,
                            y: 0,
                            width: current_vp_w as i32,
                            height: current_vp_h as i32,
                        }));
                    }
                    cmds.push(Command::SetRenderTarget(current_target_image));
                    if !preserve_contents {
                        cmds.push(Command::ClearColor { rgb: clear_rgb });
                    }
                    if current_target_image.is_none() {
                        cmds.push(Command::Present);
                    }
                    if !check_submit_budget(0, cmds.len(), "end_frame_clear_only") {
                        return -11;
                    }
                    let submit_res = ctx.submit(CommandBuffer {
                        commands: cmds.as_slice(),
                    });
                    if submit_res.is_ok() {
                        submitted_passes = submitted_passes.saturating_add(1);
                        return 0;
                    }
                    if let Err(e) = submit_res {
                        crate::globalog::log(format_args!("gfx-cabi: submit failed: {:?}\n", e));
                    }
                    return -11;
                }
                0
            })
        }) else {
            return -13;
        };

        if ret == 0 {
            let mut st = GFX_CABI_STATE.lock();
            if submitted_passes != 0 {
                st.ring_idx = (st.ring_idx + submitted_passes) % GFX_CABI_VBUF_RING_LEN;
            }
            if is_screen_present_frame {
                st.base_cache_valid = true;
                st.base_cache_updated_at_ticks = embassy_time_driver::now();
                let (screen_w, screen_h) = screenshot_overlay_extent
                    .unwrap_or((st.swapchain_desc.extent.width, st.swapchain_desc.extent.height));
                st.base_cache_screen_width = screen_w;
                st.base_cache_screen_height = screen_h;
                st.base_cache_clear_rgb = clear_rgb;
                st.base_cache_draws = draws.clone();
                st.base_cache_rgb_blob = rgb_src.clone();
                st.base_cache_tex_blob = tex_src.clone();
            }

            if crate::gfx::is_virgl_active() {
                let first = !crate::logflag::GFX_CABI_VIRGL_FIRST_FRAME_SEEN
                    .swap(true, core::sync::atomic::Ordering::AcqRel);
                if first {
                    crate::r::readiness::set(crate::r::readiness::GFX_VIRGL_READY);
                    crate::globalog::log(format_args!(
                        "gfx: virgl first frame ready seq={} bytes={}\n",
                        seq, draw_bytes
                    ));
                }
                let n = crate::logflag::GFX_CABI_VIRGL_END_FRAME_DIAG_LOGS
                    .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                if crate::logflag::GFX_FRAME_PROGRESS_LOGS && (first || n < 12) {
                    crate::globalog::log(format_args!(
                        "gfx-cabi: virgl end_frame ok seq={} rgb={} tex={} bytes={} first={}\n",
                        seq, rgb_draws, tex_draws, draw_bytes, first as u8
                    ));
                }
            }
            drop(st);

            if is_screen_present_frame {
                let mut screenshot_draws = draws.clone();
                let mut screenshot_rgb = rgb_src.clone();
                let mut screenshot_tex = tex_src.clone();
                if let Some((vp_w, vp_h)) = screenshot_overlay_extent {
                    append_kernel_cursor_overlay_draws(
                        &mut screenshot_draws,
                        &mut screenshot_rgb,
                        &mut screenshot_tex,
                        vp_w,
                        vp_h,
                        None,
                    );
                }
                maybe_publish_composed_screenshot(
                    preserve_contents,
                    clear_rgb,
                    screenshot_draws.as_slice(),
                    screenshot_rgb.as_slice(),
                    screenshot_tex.as_slice(),
                );
            }
        } else if crate::gfx::is_virgl_active() {
            let n = crate::logflag::GFX_CABI_VIRGL_END_FRAME_DIAG_LOGS
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if crate::logflag::GFX_FRAME_PROGRESS_LOGS && n < 12 {
                crate::globalog::log(format_args!(
                    "gfx-cabi: virgl end_frame failed seq={} rgb={} tex={} bytes={} rc={}\n",
                    seq, rgb_draws, tex_draws, draw_bytes, ret
                ));
            }
        }

        if crate::logflag::GFX_CABI_FRAME_DEBUG_LOGS && (seq <= 10 || (seq % 20) == 0) {
            crate::globalog::log(format_args!(
                "gfx-cabi: end seq={} rgb={} tex={} bytes={} rc={}\n",
                seq, rgb_draws, tex_draws, draw_bytes, ret
            ));
        }
        ret
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
        let Some(m) = crate::usb2::input::pop_mouse_event() else {
            return 0;
        };
        *out_buttons = m.buttons;
        *out_dx = m.dx;
        *out_dy = m.dy;
        *out_wheel = m.wheel;
        1
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_pop_tablet(
        out: *mut crate::usb2::input::TabletEvent,
    ) -> i32 {
        if out.is_null() {
            return -1;
        }
        let Some(t) = crate::usb2::input::pop_tablet_event() else {
            return 0;
        };
        (*out).slot_id = t.slot_id;
        (*out).buttons = t.buttons;
        (*out).report_id = t.report_id;
        (*out).x_raw = t.x_raw;
        (*out).y_raw = t.y_raw;
        (*out).x_norm_q15 = t.x_norm_q15;
        (*out).y_norm_q15 = t.y_norm_q15;
        (*out).flags = t.flags;
        1
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_input_keyboard_count() -> u32 {
        crate::r::keyboard::keyboard_count()
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_keyboard_modifiers(
        keyboard_id: u32,
        out_modifiers: *mut u8,
    ) -> i32 {
        if out_modifiers.is_null() {
            return -1;
        }
        let Some(modifiers) = crate::r::keyboard::keyboard_modifiers(keyboard_id) else {
            return 1;
        };
        *out_modifiers = modifiers;
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_keyboard_keys(
        keyboard_id: u32,
        out_keys: *mut u8,
        out_ascii: *mut u8,
    ) -> i32 {
        if out_keys.is_null() || out_ascii.is_null() {
            return -1;
        }
        let Some(state) = crate::r::keyboard::keyboard_state(keyboard_id) else {
            return 1;
        };
        core::ptr::copy_nonoverlapping(state.keys.as_ptr(), out_keys, state.keys.len());
        core::ptr::copy_nonoverlapping(state.ascii.as_ptr(), out_ascii, state.ascii.len());
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_pop_keyboard_output(
        out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
    ) -> i32 {
        if out.is_null() {
            return -1;
        }
        let Some(evt) = crate::r::keyboard::pop_output_event() else {
            return 0;
        };
        *out = evt;
        1
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_read_keyboard_output_since(
        read_seq: u64,
        out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
        out_cap: u32,
        out_next_seq: *mut u64,
        out_dropped: *mut u32,
    ) -> u32 {
        let mut next_seq = read_seq;
        let mut dropped = 0u32;
        let wrote = if out.is_null() || out_cap == 0 {
            0usize
        } else {
            let out_slice = core::slice::from_raw_parts_mut(out, out_cap as usize);
            let (next, lost, written) =
                crate::r::keyboard::read_output_events_since(read_seq, out_slice);
            next_seq = next;
            dropped = lost;
            written
        };

        if !out_next_seq.is_null() {
            *out_next_seq = next_seq;
        }
        if !out_dropped.is_null() {
            *out_dropped = dropped;
        }
        wrote as u32
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_write_keyboard_text(
        slot_id: u32,
        text_ptr: *const u8,
        text_len: usize,
        flags: u32,
    ) -> i32 {
        if slot_id == 0 || text_ptr.is_null() || text_len == 0 {
            return -1;
        }
        let bytes = core::slice::from_raw_parts(text_ptr, text_len);
        let Ok(text) = core::str::from_utf8(bytes) else {
            return -2;
        };
        crate::r::keyboard::inject_text(slot_id, text, flags) as i32
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_input_write_keyboard_key(
        slot_id: u32,
        codepoint: u32,
        key_code: u32,
        modifiers: u32,
        flags: u32,
    ) -> i32 {
        let key_code = if key_code > u16::MAX as u32 {
            return -2;
        } else {
            key_code as u16
        };
        let modifiers = (modifiers & 0xff) as u8;
        if crate::r::keyboard::inject_key(slot_id, codepoint, key_code, modifiers, flags) {
            1
        } else {
            0
        }
    }
}
