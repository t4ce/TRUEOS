extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

pub(crate) fn runtime_context_key() -> u32 {
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return 0x8000_0000 | vm_id as u32;
    }
    crate::percpu::this_cpu().cpu_index()
}

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
        crate::r::path::FsPath::parse(path, allow_empty)
            .map(|path| path.to_relative_string())
            .map_err(|_| FsError::BadPath)
    }

    /// Compatibility boundary for APIs whose public contract is synchronous.
    ///
    /// The caller must already be on a background AP service lane. Only typed,
    /// owned request data crosses to the BSP; the BSP creates and awaits the
    /// actual TRUEOSFS future. Never restore `spawn_and_wait_local` here: doing
    /// so recursively polls the same executor and was the USB/video stall.
    fn wait_for_filesystem<T>(
        request: core::result::Result<
            Result<T>,
            crate::r::fs::request_broker::BlockingRequestError,
        >,
    ) -> Result<T> {
        match request {
            Ok(result) => result,
            Err(error) => {
                crate::log_error!(target: "filesystem";
                    "kfs: blocking request rejected reason={:?} cpu={} executor_poll={}\n",
                    error,
                    crate::percpu::this_cpu().cpu_index(),
                    crate::percpu::in_executor_poll(),
                );
                Err(FsError::Device(block::Error::NotReady))
            }
        }
    }

    #[inline]
    pub fn read_file(path: &str) -> Result<Vec<u8>> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        wait_for_filesystem(crate::r::fs::request_broker::read_file(disk, name))
    }

    #[inline]
    pub fn read_file_len(path: &str) -> Result<usize> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        wait_for_filesystem(crate::r::fs::request_broker::read_file_len(disk, name))
    }

    #[inline]
    pub fn read_file_range(path: &str, offset: u64, out: &mut [u8]) -> Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }

        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        let cap = out.len();
        let bytes = wait_for_filesystem(crate::r::fs::request_broker::read_file_range(
            disk, name, offset, cap,
        ))?;
        let got = core::cmp::min(bytes.len(), out.len());
        out[..got].copy_from_slice(&bytes[..got]);
        Ok(got)
    }

    #[inline]
    pub fn stat(path: &str) -> Result<FsStat> {
        let disk = root_disk()?;
        let name = normalize_rel(path, true)?;
        wait_for_filesystem(crate::r::fs::request_broker::stat(disk, name))
    }

    #[inline]
    pub fn write_file_begin(path: &str, total_len: u64) -> Result<u32> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        wait_for_filesystem(crate::r::fs::request_broker::write_file_begin(disk, name, total_len))
    }

    #[inline]
    pub fn create_dir_all(path: &str) -> Result<()> {
        crate::log_warn!(target: "filesystem";
            "kfs: synchronous create_dir_all compatibility path used path={} action=continue migrate=async-fs\n",
            path,
        );
        let disk = root_disk()?;
        let name = normalize_rel(path, true)?;
        if name.is_empty() {
            return Ok(());
        }

        wait_for_filesystem(crate::r::fs::request_broker::create_dir_all(disk, name))
    }

    #[inline]
    pub fn write_file_chunk(handle: u32, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        wait_for_filesystem(crate::r::fs::request_broker::write_file_chunk(handle, data))
    }

    #[inline]
    pub fn write_file_finish(handle: u32) -> Result<()> {
        wait_for_filesystem(crate::r::fs::request_broker::write_file_finish(handle))
    }

    #[inline]
    pub fn write_file_abort(handle: u32) -> Result<()> {
        wait_for_filesystem(crate::r::fs::request_broker::write_file_abort(handle))
    }

    #[inline]
    pub fn html_tree(max_entries: usize) -> Result<String> {
        let disk = root_disk()?;
        wait_for_filesystem(crate::r::fs::request_broker::html_tree(disk, max_entries))
    }

    #[inline]
    pub fn json_all(max_entries: usize) -> Result<String> {
        let disk = root_disk()?;
        wait_for_filesystem(crate::r::fs::request_broker::json_all(disk, max_entries))
    }

    #[inline]
    pub fn list_dir(path: &str) -> Result<String> {
        let stat = stat(path)?;
        if stat.kind != FsNodeKind::Directory {
            return Err(FsError::BadPath);
        }

        let disk = root_disk()?;
        let name = normalize_rel(path, true)?;
        wait_for_filesystem(crate::r::fs::request_broker::list_dir(disk, name))
    }

    #[inline]
    pub fn remove(path: &str) -> Result<()> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        wait_for_filesystem(crate::r::fs::request_broker::remove(disk, name))
    }

    #[inline]
    pub fn exists(path: &str) -> Result<bool> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        wait_for_filesystem(crate::r::fs::request_broker::exists(disk, name))
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

    fn insert_kernel_locale_env(vars: &mut BTreeMap<String, String>) {
        for key in [
            "LANG",
            "LANGUAGE",
            "TRUEOS_LANGUAGE",
            "LC_ALL",
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MESSAGES",
            "LC_MONETARY",
            "LC_NUMERIC",
            "LC_TIME",
            "TRUEOS_LOCALE",
            "TZ",
            "TRUEOS_TIMEZONE",
        ] {
            if let Some(value) = crate::locale::env_var(key) {
                vars.entry(String::from(key)).or_insert(String::from(value));
            }
        }
    }

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

    /// Returns the VM-owned process context when execution has crossed from a
    /// Hull into a host service lane.
    ///
    /// Hull RW/BSS is private to the guest, so the launch-context stack pushed
    /// there is not the same `CONTEXTS` storage observed by a Tokio carrier.
    /// The carrier retains its VM identity through `KernelTaskDomain`, making
    /// the host `BLUEPRINT_PROCESS_CONTEXTS` table the authoritative
    /// cross-realm fallback.
    fn process_vm_id() -> Option<u8> {
        crate::hv::current_guest_execution_context_vm_id()
    }

    pub(crate) fn with_launch_context_console_and_fs_root<R>(
        args: Vec<String>,
        vars: BTreeMap<String, String>,
        console_target: Option<MatrixTarget>,
        app_fs_root: Option<String>,
        f: impl FnOnce() -> R,
    ) -> R {
        let mut vars = vars;
        insert_kernel_locale_env(&mut vars);
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
        let local = {
            let stack = context_stack().lock();
            stack.last().map(|ctx| ctx.args.len())
        };
        local
            .or_else(|| process_vm_id().and_then(crate::hv::blueprint_process_arg_count))
            .unwrap_or(0)
    }

    pub fn arg(index: usize) -> Option<String> {
        let local = {
            let stack = context_stack().lock();
            stack.last().map(|ctx| ctx.args.get(index).cloned())
        };
        match local {
            Some(arg) => arg,
            None => {
                process_vm_id().and_then(|vm_id| crate::hv::blueprint_process_arg(vm_id, index))
            }
        }
    }

    pub fn var(key: &str) -> Option<String> {
        let local = {
            let stack = context_stack().lock();
            stack.last().map(|ctx| ctx.vars.get(key).cloned())
        };
        match local {
            Some(value) => value,
            None => {
                process_vm_id().and_then(|vm_id| crate::hv::blueprint_process_env_var(vm_id, key))
            }
        }
        .or_else(|| crate::locale::env_var(key).map(String::from))
    }

    pub(crate) fn current_app_fs_root() -> Option<String> {
        let local = {
            let stack = context_stack().lock();
            stack.last().map(|ctx| ctx.app_fs_root.clone())
        };
        match local {
            Some(root) => root,
            None => process_vm_id().and_then(|vm_id| {
                crate::hv::blueprint_process_env_var(vm_id, "TRUEOS_APP_FS_ROOT")
            }),
        }
    }

    pub(crate) fn trueosfs_scope_granted() -> bool {
        var("TRUEOS_FS_SCOPE").as_deref() == Some("trueosfs")
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

    pub(crate) fn retarget_console_slot(requested: &str) -> bool {
        let mut stack = context_stack().lock();
        let Some(ctx) = stack.last_mut() else {
            return false;
        };
        let next_target = match ctx.console_target.as_ref() {
            Some(target) => crate::shell2::switch_matrix_target_slot(target, requested),
            None => crate::shell2::matrix_target_for_slot_name(
                crate::shell2::OUTPUT_NET_TCP_MASK,
                requested,
            ),
        };
        ctx.console_target = Some(next_target);
        true
    }

    fn normalize_app_path(path: &str, allow_empty: bool) -> Option<String> {
        crate::r::path::FsPath::parse(path, allow_empty)
            .ok()
            .map(|path| path.to_relative_string())
    }

    pub(crate) fn resolve_fs_path(path: &str, allow_empty: bool) -> Option<String> {
        if trueosfs_scope_granted() {
            return normalize_app_path(path, allow_empty);
        }
        let Some(root) = current_app_fs_root() else {
            return Some(String::from(path));
        };

        let rel = normalize_app_path(path, allow_empty)?;
        let root_rel = normalize_app_path(root.as_str(), true)?;
        if rel.is_empty() || rel == root_rel {
            Some(root)
        } else if let Some(app_rel) = rel.strip_prefix(root_rel.as_str()) {
            let app_rel = app_rel.strip_prefix('/').unwrap_or(app_rel);
            if app_rel.is_empty() {
                Some(root)
            } else {
                Some(alloc::format!("{}/{}", root.trim_matches('/'), app_rel))
            }
        } else if rel == "common" || rel == "apps/common" {
            Some(String::from("apps/common"))
        } else if let Some(shared_rel) = rel
            .strip_prefix("common/")
            .or_else(|| rel.strip_prefix("apps/common/"))
        {
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

pub mod async_fs_cabi;
pub mod calculator_cabi;
pub mod fs_cabi;
pub mod input_cabi;
pub mod vgpu_cabi;

pub mod cabi {
    pub use super::async_fs_cabi::{
        trueos_cabi_async_fs_create_dir_all_start, trueos_cabi_async_fs_discard,
        trueos_cabi_async_fs_list_dir_start, trueos_cabi_async_fs_list_mounts_start,
        trueos_cabi_async_fs_read_start, trueos_cabi_async_fs_remove_start,
        trueos_cabi_async_fs_result_len, trueos_cabi_async_fs_result_read,
        trueos_cabi_async_fs_stat_start, trueos_cabi_async_fs_status,
        trueos_cabi_async_fs_write_begin, trueos_cabi_async_fs_write_chunk,
        trueos_cabi_async_fs_write_commit,
    };
    pub use super::calculator_cabi::*;
    pub use super::fs_cabi::*;
    pub use super::input_cabi::*;
    pub use super::vgpu_cabi::*;
    pub use crate::r::net::https::{
        trueos_cabi_net_fetch_bytes_discard, trueos_cabi_net_fetch_bytes_read,
        trueos_cabi_net_fetch_bytes_result_len, trueos_cabi_net_fetch_bytes_start,
        trueos_cabi_net_fetch_bytes_wait, trueos_cabi_net_fetch_discard,
        trueos_cabi_net_fetch_post_json_bytes_start,
        trueos_cabi_net_fetch_post_json_bytes_start_with_timeout,
        trueos_cabi_net_fetch_post_json_start, trueos_cabi_net_fetch_post_json_start_with_timeout,
        trueos_cabi_net_fetch_result, trueos_cabi_net_fetch_start, trueos_cabi_net_fetch_wait,
        trueos_cabi_net_prewarm_url_start,
    };
    pub use crate::r::net::socket_cabi::{
        trueos_cabi_tun_close, trueos_cabi_tun_open, trueos_cabi_tun_recv, trueos_cabi_tun_send,
    };
}
