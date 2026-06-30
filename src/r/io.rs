extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

fn runtime_context_key() -> u32 {
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

            if crate::r::fs::trueosfs::dir_has_children_async(disk, name.as_str()).await? {
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
                if crate::r::fs::trueosfs::file_exists_async(disk, marker.as_str()).await?
                    || crate::r::fs::trueosfs::dir_has_children_async(disk, prefix.as_str()).await?
                {
                    continue;
                }

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
            match crate::r::fs::fs_html::html_tree_async(disk, max_entries).await? {
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
    pub fn list_dir(path: &str) -> Result<String> {
        let stat = stat(path)?;
        if stat.kind != FsNodeKind::Directory {
            return Err(FsError::BadPath);
        }

        let disk = root_disk()?;
        let name = normalize_rel(path, true)?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::r::fs::trueosfs::list_dir_async(disk, name.as_str()).await? {
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
        let stack = context_stack().lock();
        stack.last().map(|ctx| ctx.args.len()).unwrap_or(0)
    }

    pub fn arg(index: usize) -> Option<String> {
        let stack = context_stack().lock();
        stack.last().and_then(|ctx| ctx.args.get(index)).cloned()
    }

    pub fn var(key: &str) -> Option<String> {
        let stack = context_stack().lock();
        stack
            .last()
            .and_then(|ctx| ctx.vars.get(key).cloned())
            .or_else(|| crate::locale::env_var(key).map(String::from))
    }

    pub(crate) fn current_app_fs_root() -> Option<String> {
        let stack = context_stack().lock();
        stack.last().and_then(|ctx| ctx.app_fs_root.clone())
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
                crate::shell2::OUTPUT_UART1_MASK,
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
        let stack = context_stack().lock();
        let app_fs_root = stack.last().and_then(|ctx| ctx.app_fs_root.clone());
        drop(stack);

        let Some(root) = app_fs_root else {
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

pub mod fs_cabi;
pub mod input_cabi;
pub mod ui3_cabi;

pub mod cabi {
    pub use super::fs_cabi::*;
    pub use super::input_cabi::*;
    pub use super::ui3_cabi::*;
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
}
