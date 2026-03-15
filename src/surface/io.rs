extern crate alloc;

use alloc::vec::Vec;
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
            match crate::v::fs::trueosfs::file_out_async(disk, name.as_str()).await? {
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
            match crate::v::fs::trueosfs::file_info_async(disk, name.as_str()).await? {
                Some(info) => Ok(info.data_len as usize),
                None => Err(FsError::NotFound),
            }
        })
    }

    #[inline]
    pub fn write_file_begin(path: &str, total_len: u64) -> Result<u32> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::v::fs::trueosfs::file_write_begin_async(disk, name.as_str(), total_len)
                .await?
            {
                Some(h) => Ok(h),
                None => Err(FsError::NoSpace),
            }
        })
    }

    #[inline]
    pub fn write_file_chunk(handle: u32, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        crate::wait::spawn_and_wait_local(async move {
            crate::v::fs::trueosfs::file_write_chunk_async(handle, data.as_slice()).await?;
            Ok(())
        })
    }

    #[inline]
    pub fn write_file_finish(handle: u32) -> Result<()> {
        crate::wait::spawn_and_wait_local(async move {
            crate::v::fs::trueosfs::file_write_finish_async(handle).await?;
            Ok(())
        })
    }

    #[inline]
    pub fn write_file_abort(handle: u32) -> Result<()> {
        crate::wait::spawn_and_wait_local(async move {
            crate::v::fs::trueosfs::file_write_abort_async(handle).await?;
            Ok(())
        })
    }

    #[inline]
    pub fn html_tree(max_entries: usize) -> Result<String> {
        let disk = root_disk()?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::v::fs::trueosfs::html_tree_async(disk, max_entries).await? {
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
            let ok = crate::v::fs::trueosfs::file_delete_async(disk, name.as_str()).await?;
            if ok { Ok(()) } else { Err(FsError::NotFound) }
        })
    }

    #[inline]
    pub fn exists(path: &str) -> Result<bool> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            Ok(crate::v::fs::trueosfs::file_exists_async(disk, name.as_str()).await?)
        })
    }
}

/// Console routing + C ABI entrypoints used by embedded C code (QuickJS etc).
pub mod cabi {
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

    #[unsafe(no_mangle)]
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

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_uart1_shell_write(
        data_ptr: *const u8,
        data_len: usize,
    ) -> usize {
        if data_ptr.is_null() || data_len == 0 {
            return 0;
        }
        let data = core::slice::from_raw_parts(data_ptr, data_len);
        crate::shell::uart1_com1::write_bytes(data);
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
        crate::shell::uart1_com1::inject_bytes(data)
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_poll_once() {
        crate::wait::spin_step();
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
        crate::v::net::ntp::current_unix_seconds().unwrap_or(0)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_ntp_kernel_date_day_month_year(
        out_ptr: *mut u8,
        out_len: usize,
    ) -> usize {
        let s = crate::v::net::ntp::kernel_date_day_month_year();
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

    static CABI_ALLOC_TABLE: spin::Mutex<alloc::collections::BTreeMap<usize, AllocMeta>> =
        spin::Mutex::new(alloc::collections::BTreeMap::new());

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

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_alloc(size: usize) -> *mut u8 {
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
        CABI_ALLOC_TABLE
            .lock()
            .get(&(ptr as usize))
            .map(|m| m.size)
            .unwrap_or(0)
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

        if out_ptr.is_null() || out_cap == 0 {
            return match super::kfs::read_file_len(path) {
                Ok(len) => len as isize,
                Err(e) => fs_error_to_code(e) as isize,
            };
        }

        match super::kfs::read_file(path) {
            Ok(bytes) => {
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
        match super::kfs::write_file_begin(path, total_len) {
            Ok(h) => {
                *out_handle = h;
                0
            }
            Err(e) => fs_error_to_code(e),
        }
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
        match super::kfs::write_file_chunk(handle, data) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_write_finish(handle: u32) -> i32 {
        match super::kfs::write_file_finish(handle) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_write_abort(handle: u32) -> i32 {
        match super::kfs::write_file_abort(handle) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
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
        match super::kfs::remove(path) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
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

    // --- GFX C-ABI ---
    // This is the stable bridge between the in-kernel JS "WebGL" shim and the renderer.
    // It intentionally targets the gfx abstraction (`trueos_gfx_core`) rather than a GPU driver.

    use crate::usb;
    use alloc::collections::VecDeque;
    use alloc::vec::Vec;
    use embassy_time::Timer;
    use trueos_gfx_core::{
        BlendDesc, BlendFactor, BufferDesc, BufferId, BufferUsage, ColorFormat, Command,
        CommandBuffer, Extent2D, GfxContext, ImageDesc, ImageFormat, ImageId, MemoryType,
        PipelineDesc, PipelineId, SamplerDesc, SamplerFilter, SamplerWrap,
        ScissorRect as GfxScissorRect, ShaderId, SwapchainDesc, TexCoordFormat, VertexLayout,
        Viewport,
    };

    const GFX_CABI_VBUF_RING_LEN: usize = 3;
    // Shared draw chunk budget used by cmd-stream draw capture paths.
    const MAX_CMDSTREAM_DRAW_BYTES: usize = 64 * 1024;
    // Conservative pre-submit guard to avoid submit_3d request overflow.
    const MAX_EST_SUBMIT_BYTES: usize = 512 * 1024;
    static SUBMIT_BUDGET_LOGS: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(0);
    static VIRGL_END_FRAME_DIAG_LOGS: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(0);
    static VIRGL_FIRST_FRAME_SEEN: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
    const TEX_PIPELINE_FS_MASK_TAG_RAW: u32 = 0x4D41_534B;
    const TEX_PIPELINE_FS_RGBA_TAG_RAW: u32 = 0x5247_4241;
    const ASYNC_TEX_STATUS_UNKNOWN: i32 = 0;
    const ASYNC_TEX_STATUS_PENDING: i32 = 1;
    const ASYNC_TEX_STATUS_READY: i32 = 2;
    static ASYNC_TEX_STATUS: spin::Mutex<Vec<i32>> = spin::Mutex::new(Vec::new());
    static ASYNC_PNG_REQS: spin::Mutex<VecDeque<AsyncPngUploadReq>> =
        spin::Mutex::new(VecDeque::new());
    static ASYNC_SVG_REQS: spin::Mutex<VecDeque<AsyncSvgUploadReq>> =
        spin::Mutex::new(VecDeque::new());
    static TEXTURE_UPLOAD_REQS: spin::Mutex<VecDeque<TextureUploadReq>> =
        spin::Mutex::new(VecDeque::new());
    static ASYNC_PNG_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
    static ASYNC_SVG_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
    static TEXTURE_UPLOAD_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
    static ASYNC_PNG_WORKER_STARTED: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
    static ASYNC_SVG_WORKER_STARTED: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);

    struct AsyncPngUploadReq {
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
        rgba: Vec<u8>,
        sample_kind: TexSampleKind,
        repaint_window_id: u32,
        repaint_reason: &'static str,
        update_async_status: bool,
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

    fn enqueue_async_png_upload(tex_id: u32, bytes: Vec<u8>) {
        ASYNC_PNG_REQS
            .lock()
            .push_back(AsyncPngUploadReq { tex_id, bytes });
        ASYNC_PNG_WAIT.notify_one();
    }

    fn enqueue_async_svg_upload(tex_id: u32, bytes: Vec<u8>) {
        ASYNC_SVG_REQS
            .lock()
            .push_back(AsyncSvgUploadReq { tex_id, bytes });
        ASYNC_SVG_WAIT.notify_one();
    }

    fn take_async_png_upload() -> Option<AsyncPngUploadReq> {
        ASYNC_PNG_REQS.lock().pop_front()
    }

    fn take_async_svg_upload() -> Option<AsyncSvgUploadReq> {
        ASYNC_SVG_REQS.lock().pop_front()
    }

    fn enqueue_texture_upload(req: TextureUploadReq) {
        let mut queue = TEXTURE_UPLOAD_REQS.lock();
        if let Some(existing) = queue.iter_mut().find(|entry| entry.tex_id == req.tex_id) {
            *existing = req;
        } else {
            queue.push_back(req);
        }
        TEXTURE_UPLOAD_WAIT.notify_one();
    }

    fn take_texture_upload() -> Option<TextureUploadReq> {
        TEXTURE_UPLOAD_REQS.lock().pop_front()
    }

    fn queue_texture_rgba_upload_owned(
        tex_id: u32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        sample_kind: TexSampleKind,
        repaint_window_id: u32,
        repaint_reason: &'static str,
        update_async_status: bool,
    ) -> bool {
        if tex_id == 0 || width == 0 || height == 0 {
            return false;
        }
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if rgba.len() < expected {
            return false;
        }
        enqueue_texture_upload(TextureUploadReq {
            tex_id,
            width,
            height,
            rgba,
            sample_kind,
            repaint_window_id,
            repaint_reason,
            update_async_status,
        });
        true
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
            rgba.to_vec(),
            TexSampleKind::Rgba,
            repaint_window_id,
            repaint_reason,
            false,
        )
    }

    async fn texture_upload_service_inner() {
        loop {
            let Some(req) = take_texture_upload() else {
                TEXTURE_UPLOAD_WAIT.wait_for_event().await;
                continue;
            };
            let rc = upload_texture_rgba_inner(
                req.tex_id,
                req.width,
                req.height,
                req.rgba.as_ptr(),
                req.rgba.len(),
                req.sample_kind,
            );
            if req.update_async_status {
                if rc == 0 {
                    set_async_tex_status(req.tex_id, ASYNC_TEX_STATUS_READY);
                } else {
                    set_async_tex_status(req.tex_id, rc);
                }
            }
            if rc == 0 && req.repaint_window_id != 0 {
                let _ = crate::v::ui2::request_window_repaint(
                    req.repaint_window_id,
                    req.repaint_reason,
                );
            }
            Timer::after_millis(1).await;
        }
    }

    #[embassy_executor::task]
    pub async fn texture_upload_service_task() {
        texture_upload_service_inner().await;
    }

    async fn async_png_decode_upload_inner(tex_id: u32, bytes: Vec<u8>) {
        let rc = match crate::gfx::png_codec::decode_png_rgba(bytes.as_slice()) {
            Ok(decoded) => {
                if queue_texture_rgba_upload_owned(
                    tex_id,
                    decoded.width,
                    decoded.height,
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

    async fn async_png_upload_service_inner() {
        loop {
            let Some(req) = take_async_png_upload() else {
                ASYNC_PNG_WAIT.wait_for_event().await;
                continue;
            };
            async_png_decode_upload_inner(req.tex_id, req.bytes).await;
            Timer::after_millis(1).await;
        }
    }

    #[embassy_executor::task]
    async fn async_png_upload_service_task() {
        async_png_upload_service_inner().await;
    }

    async fn async_svg_decode_upload_inner(tex_id: u32, bytes: Vec<u8>) {
        let rc = match crate::gfx::svg::upload_svg_bytes_to_texture(tex_id, bytes.as_slice()) {
            Ok(_) => 0,
            Err(code) => code,
        };
        if rc == 0 {
            set_async_tex_status(tex_id, ASYNC_TEX_STATUS_READY);
        } else {
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

    fn try_start_async_png_worker() {
        if ASYNC_PNG_WORKER_STARTED.load(core::sync::atomic::Ordering::Acquire) {
            return;
        }

        if let Some(worker_spawner) = trueos_qjs::workers::pick_background_spawner() {
            if ASYNC_PNG_WORKER_STARTED
                .compare_exchange(
                    false,
                    true,
                    core::sync::atomic::Ordering::AcqRel,
                    core::sync::atomic::Ordering::Acquire,
                )
                .is_ok()
            {
                if worker_spawner
                    .spawn(async_png_upload_service_task())
                    .is_err()
                {
                    ASYNC_PNG_WORKER_STARTED.store(false, core::sync::atomic::Ordering::Release);
                }
            }
            return;
        }

        if crate::smp::cpu_count() > 1 {
            crate::globalog::log(format_args!(
                "async-png: no background spawner available on multicore system; worker not started\n"
            ));
        }

        if crate::smp::cpu_count() <= 1
            && ASYNC_PNG_WORKER_STARTED
                .compare_exchange(
                    false,
                    true,
                    core::sync::atomic::Ordering::AcqRel,
                    core::sync::atomic::Ordering::Acquire,
                )
                .is_ok()
        {
            crate::wait::spawn_local_detached(async move {
                async_png_upload_service_inner().await;
            });
        }
    }

    fn try_start_async_svg_worker() {
        if ASYNC_SVG_WORKER_STARTED.load(core::sync::atomic::Ordering::Acquire) {
            return;
        }

        if let Some(worker_spawner) = trueos_qjs::workers::pick_background_spawner() {
            if ASYNC_SVG_WORKER_STARTED
                .compare_exchange(
                    false,
                    true,
                    core::sync::atomic::Ordering::AcqRel,
                    core::sync::atomic::Ordering::Acquire,
                )
                .is_ok()
            {
                if worker_spawner
                    .spawn(async_svg_upload_service_task())
                    .is_err()
                {
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
        tex_vbuf: [BufferId; GFX_CABI_VBUF_RING_LEN],
        tex_capacity: [usize; GFX_CABI_VBUF_RING_LEN],
        tex_images: Option<Vec<Option<TexImage>>>,
        epoch: u64,
        swapchain_configured: bool,
        swapchain_desc: SwapchainDesc,
        viewport_configured: bool,
        frame_active: bool,
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
        last_missing_tex_id: u32,
        missing_tex_logs: u32,
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
    enum TexCoordOrigin {
        TopLeft,
        BottomLeft,
    }

    struct TexImage {
        image: ImageId,
        width: u32,
        height: u32,
        sample_kind: TexSampleKind,
        origin: TexCoordOrigin,
    }

    #[derive(Clone, Copy)]
    enum PendingDraw {
        SetRenderTarget {
            tex_id: u32,
        },
        SetScissor {
            rect: Option<ScissorRect>,
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
                last_missing_tex_id: 0,
                missing_tex_logs: 0,
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

    #[inline]
    fn frame_target_extent(st: &GfxCabiState) -> (u32, u32) {
        let tex_id = st.frame_render_target_tex_id;
        if tex_id != 0 {
            let idx = tex_id.saturating_sub(1) as usize;
            if let Some((w, h)) = st
                .tex_images
                .as_ref()
                .and_then(|images| images.get(idx))
                .and_then(|entry| entry.as_ref())
                .map(|img| (img.width, img.height))
            {
                return (w.max(1), h.max(1));
            }
        }
        (
            st.swapchain_desc.extent.width.max(1),
            st.swapchain_desc.extent.height.max(1),
        )
    }

    fn texture_dimensions_inner(tex_id: u32) -> Option<(u32, u32)> {
        if tex_id == 0 {
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

    #[derive(Clone, Copy)]
    struct RgbVtx {
        x: f32,
        y: f32,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
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
    fn read_rgb_vtx(bytes: &[u8], off: usize) -> Option<RgbVtx> {
        if off + 12 > bytes.len() {
            return None;
        }
        let x = f32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
        let y = f32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]);
        Some(RgbVtx {
            x,
            y,
            r: (bytes[off + 8] as f32) / 255.0,
            g: (bytes[off + 9] as f32) / 255.0,
            b: (bytes[off + 10] as f32) / 255.0,
            a: (bytes[off + 11] as f32) / 255.0,
        })
    }

    #[inline]
    fn push_rgb_vtx(out: &mut Vec<u8>, v: RgbVtx) {
        out.extend_from_slice(&v.x.to_le_bytes());
        out.extend_from_slice(&v.y.to_le_bytes());
        out.push((clamp01(v.r) * 255.0 + 0.5) as u8);
        out.push((clamp01(v.g) * 255.0 + 0.5) as u8);
        out.push((clamp01(v.b) * 255.0 + 0.5) as u8);
        out.push((clamp01(v.a) * 255.0 + 0.5) as u8);
    }

    #[inline]
    fn interp_rgb(a: RgbVtx, b: RgbVtx, t: f32) -> RgbVtx {
        RgbVtx {
            x: lerp(a.x, b.x, t),
            y: lerp(a.y, b.y, t),
            r: lerp(a.r, b.r, t),
            g: lerp(a.g, b.g, t),
            b: lerp(a.b, b.b, t),
            a: lerp(a.a, b.a, t),
        }
    }

    #[inline]
    fn scissor_to_ndc(scissor: ScissorRect, vp_w: u32, vp_h: u32) -> Option<(f32, f32, f32, f32)> {
        if vp_w == 0 || vp_h == 0 {
            return None;
        }
        let x0 = scissor.x.min(vp_w) as f32;
        let y0 = scissor.y.min(vp_h) as f32;
        let x1 = scissor.x.saturating_add(scissor.width).min(vp_w) as f32;
        let y1 = scissor.y.saturating_add(scissor.height).min(vp_h) as f32;
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let w = vp_w as f32;
        let h = vp_h as f32;
        let left = (x0 / w) * 2.0 - 1.0;
        let right = (x1 / w) * 2.0 - 1.0;
        let top = 1.0 - (y0 / h) * 2.0;
        let bottom = 1.0 - (y1 / h) * 2.0;
        Some((left, right, bottom, top))
    }

    fn clip_poly_edge(input: &[RgbVtx], edge: u8, bound: f32, out: &mut Vec<RgbVtx>) {
        out.clear();
        if input.is_empty() {
            return;
        }

        let mut prev = input[input.len() - 1];
        let mut prev_in = match edge {
            0 => prev.x >= bound,
            1 => prev.x <= bound,
            2 => prev.y >= bound,
            _ => prev.y <= bound,
        };

        for &cur in input {
            let cur_in = match edge {
                0 => cur.x >= bound,
                1 => cur.x <= bound,
                2 => cur.y >= bound,
                _ => cur.y <= bound,
            };

            if cur_in != prev_in {
                let denom = match edge {
                    0 | 1 => cur.x - prev.x,
                    _ => cur.y - prev.y,
                };
                if denom.abs() > 1e-6 {
                    let t = match edge {
                        0 | 1 => (bound - prev.x) / denom,
                        _ => (bound - prev.y) / denom,
                    };
                    out.push(interp_rgb(prev, cur, t));
                }
            }

            if cur_in {
                out.push(cur);
            }

            prev = cur;
            prev_in = cur_in;
        }
    }

    fn clip_rgb_triangles_to_scissor(
        src: &[u8],
        scissor: ScissorRect,
        vp_w: u32,
        vp_h: u32,
    ) -> Vec<u8> {
        const VTX_SIZE: usize = 12;
        const TRI_SIZE: usize = VTX_SIZE * 3;

        let Some((left, right, bottom, top)) = scissor_to_ndc(scissor, vp_w, vp_h) else {
            return Vec::new();
        };

        let mut out = Vec::with_capacity(src.len());
        let usable = src.len() - (src.len() % TRI_SIZE);
        let mut poly_a: Vec<RgbVtx> = Vec::with_capacity(8);
        let mut poly_b: Vec<RgbVtx> = Vec::with_capacity(8);

        let mut off = 0usize;
        while off + TRI_SIZE <= usable {
            let Some(v0) = read_rgb_vtx(src, off) else {
                break;
            };
            let Some(v1) = read_rgb_vtx(src, off + VTX_SIZE) else {
                break;
            };
            let Some(v2) = read_rgb_vtx(src, off + (2 * VTX_SIZE)) else {
                break;
            };
            off += TRI_SIZE;

            poly_a.clear();
            poly_a.push(v0);
            poly_a.push(v1);
            poly_a.push(v2);

            clip_poly_edge(&poly_a, 0, left, &mut poly_b);
            if poly_b.len() < 3 {
                continue;
            }
            clip_poly_edge(&poly_b, 1, right, &mut poly_a);
            if poly_a.len() < 3 {
                continue;
            }
            clip_poly_edge(&poly_a, 2, bottom, &mut poly_b);
            if poly_b.len() < 3 {
                continue;
            }
            clip_poly_edge(&poly_b, 3, top, &mut poly_a);
            if poly_a.len() < 3 {
                continue;
            }

            let base = poly_a[0];
            for i in 1..(poly_a.len() - 1) {
                push_rgb_vtx(&mut out, base);
                push_rgb_vtx(&mut out, poly_a[i]);
                push_rgb_vtx(&mut out, poly_a[i + 1]);
            }
        }

        out
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
        let en = enabled != 0;
        let mut st = GFX_CABI_STATE.lock();
        st.cur_blend = BlendDesc {
            enabled: en,
            src: gl_blend_factor_to_core(src_rgb),
            dst: gl_blend_factor_to_core(dst_rgb),
        };
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_set_sampler(
        wrap_s: u32,
        wrap_t: u32,
        min_filter: u32,
        mag_filter: u32,
    ) -> i32 {
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
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_set_scissor(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> i32 {
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
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_clear_scissor() -> i32 {
        let mut st = GFX_CABI_STATE.lock();
        st.cur_scissor = None;
        if st.frame_active {
            st.frame_draws.push(PendingDraw::SetScissor { rect: None });
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_set_render_target(tex_id: u32) -> i32 {
        let mut st = GFX_CABI_STATE.lock();
        if tex_id == 0 {
            st.frame_render_target_tex_id = 0;
            st.viewport_configured = false;
            if st.frame_active {
                st.frame_draws
                    .push(PendingDraw::SetRenderTarget { tex_id: 0 });
            }
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
        st.frame_render_target_tex_id = tex_id;
        st.viewport_configured = false;
        if st.frame_active {
            st.frame_draws.push(PendingDraw::SetRenderTarget { tex_id });
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_clear_render_target() -> i32 {
        let mut st = GFX_CABI_STATE.lock();
        st.frame_render_target_tex_id = 0;
        st.viewport_configured = false;
        if st.frame_active {
            st.frame_draws
                .push(PendingDraw::SetRenderTarget { tex_id: 0 });
        }
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
        let n = SUBMIT_BUDGET_LOGS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
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
        sample_kind: TexSampleKind,
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

        let mut pipeline_id = match sample_kind {
            TexSampleKind::Mask => st.tex_pipeline_mask,
            TexSampleKind::Rgba => st.tex_pipeline_rgba,
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
            let fs_tag = match sample_kind {
                TexSampleKind::Mask => ShaderId::from_raw(TEX_PIPELINE_FS_MASK_TAG_RAW),
                TexSampleKind::Rgba => ShaderId::from_raw(TEX_PIPELINE_FS_RGBA_TAG_RAW),
            };
            let p = ctx
                .create_pipeline(PipelineDesc {
                    vertex_layout: layout,
                    vs: None,
                    fs: Some(fs_tag),
                })
                .ok()?;
            pipeline_id = p;
            match sample_kind {
                TexSampleKind::Mask => st.tex_pipeline_mask = p,
                TexSampleKind::Rgba => st.tex_pipeline_rgba = p,
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
        crate::gfx::init(crate::limine::framebuffer_response());

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

    pub fn render_rgb_triangles_to_texture(tex_id: u32, clear_rgb: u32, vtx: &[u8]) -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        if tex_id == 0 {
            return -1;
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

                    let mut st = GFX_CABI_STATE.lock();
                    st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                    st.viewport_configured = false;
                    0
                },
            ) else {
                return -9;
            };
            ret
        })
    }

    fn upload_texture_rgba_inner(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
        sample_kind: TexSampleKind,
    ) -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

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

        let Some(ret) =
            crate::gfx::with_context_tag(crate::gfx::SystemLockOwner::UploadTexture, |ctx| {
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
                        return -4;
                    };
                    image_id = img;
                }
                images[idx] = Some(TexImage {
                    image: image_id,
                    width,
                    height,
                    sample_kind,
                    origin: TexCoordOrigin::TopLeft,
                });
                if ctx.write_image(image_id, data).is_err() {
                    return -5;
                }
                0
            })
        else {
            return -6;
        };
        ret
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
            data_ptr,
            data_len,
            TexSampleKind::Rgba,
        )
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_png(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

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
        crate::gfx::init(crate::limine::framebuffer_response());

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
        enqueue_async_png_upload(tex_id, bytes);
        try_start_async_png_worker();
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_svg(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        if tex_id == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        let data = core::slice::from_raw_parts(data_ptr, data_len);
        match crate::gfx::svg::upload_svg_bytes_to_texture(tex_id, data) {
            Ok(_) => 0,
            Err(code) => code,
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_svg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

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
        if get_async_tex_status(tex_id) == ASYNC_TEX_STATUS_PENDING {
            try_start_async_png_worker();
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
    fn begin_frame_inner(clear_rgb: u32, preserve_contents: bool) -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        let mut st = GFX_CABI_STATE.lock();
        // Keep CABI epoch aligned at frame start so first-use texture upload does not
        // treat initial bootstrap as a backend switch and invalidate this frame.
        st.epoch = crate::gfx::backend_epoch();
        st.frame_seq = st.frame_seq.wrapping_add(1);
        st.frame_active = true;
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
        if seq <= 10 || seq.is_multiple_of(20) {
            crate::globalog::log(format_args!(
                "gfx-cabi: begin seq={} clear=0x{:06X}\n",
                seq,
                clear_rgb & 0x00FF_FFFF
            ));
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32 {
        begin_frame_inner(clear_rgb, false)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_begin_frame_preserve(clear_rgb: u32) -> i32 {
        begin_frame_inner(clear_rgb, true)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_draw_rgb_triangles_no_present(
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32 {
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

        let clipped_owned;
        let clipped = if let Some(scissor) = st.cur_scissor {
            let (vp_w, vp_h) = frame_target_extent(&st);
            clipped_owned = clip_rgb_triangles_to_scissor(bytes, scissor, vp_w, vp_h);
            clipped_owned.as_slice()
        } else {
            bytes
        };
        if clipped.is_empty() {
            return 0;
        }

        st.frame_rgb_draws = st.frame_rgb_draws.saturating_add(1);
        st.frame_draw_bytes = st.frame_draw_bytes.saturating_add(clipped.len());
        let blend = st.cur_blend;
        let mut off = 0usize;
        while off < clipped.len() {
            let rem = clipped.len() - off;
            let chunk = core::cmp::min(MAX_CMDSTREAM_DRAW_BYTES, rem);
            let chunk = chunk - (chunk % VTX_SIZE);
            if chunk == 0 {
                break;
            }
            let blob_offset = st.frame_rgb_blob.len();
            st.frame_rgb_blob
                .extend_from_slice(&clipped[off..off + chunk]);
            st.frame_draws.push(PendingDraw::Rgb {
                blob_offset,
                blob_len: chunk,
                blend,
            });
            off += chunk;
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_draw_tex_triangles_no_present(
        tex_id: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32 {
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
            .unwrap_or((
                ImageId::invalid(),
                TexSampleKind::Mask,
                TexCoordOrigin::TopLeft,
            ));
        let sampler = st.cur_sampler;
        let blend = st.cur_blend;
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
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_end_frame() -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        let (
            seq,
            rgb_draws,
            tex_draws,
            draw_bytes,
            was_active,
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
                st.frame_preserve_contents,
                st.frame_clear_rgb,
                core::mem::take(&mut st.frame_draws),
                core::mem::take(&mut st.frame_rgb_blob),
                core::mem::take(&mut st.frame_tex_blob),
            );
            st.frame_active = false;
            st.frame_preserve_contents = false;
            st.frame_render_target_tex_id = 0;
            out
        };
        if !was_active {
            crate::globalog::log(format_args!("gfx-cabi: end without active frame\n"));
            return -3;
        }

        let mut final_render_target_tex_id = 0u32;
        for draw in &draws {
            if let PendingDraw::SetRenderTarget { tex_id } = draw {
                final_render_target_tex_id = *tex_id;
            }
        }
        let is_screen_present_frame = final_render_target_tex_id == 0;

        let Some(ret) = crate::gfx::with_context_tag(
            crate::gfx::SystemLockOwner::EndFrame,
            |ctx| {
                let (_p, _v, need_set_viewport) = match ensure_gfx_resources(ctx, 0) {
                    Some(v) => v,
                    None => return -1,
                };
                let swap = ctx.swapchain_desc();
                // Compose cursor into app-driven presents to avoid one-frame cursor blink
                // between end_frame and the async cursor overlay tick.
                let mut submit_draws = draws.clone();
                let mut submit_rgb_src = rgb_src.clone();
                let submit_tex_src = tex_src.clone();
                if is_screen_present_frame {
                    append_kernel_cursor_overlay_draws(
                        &mut submit_draws,
                        &mut submit_rgb_src,
                        swap.extent.width,
                        swap.extent.height,
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
                                    let st = GFX_CABI_STATE.lock();
                                    let idx = tex_id.saturating_sub(1) as usize;
                                    let Some(img) = st
                                        .tex_images
                                        .as_ref()
                                        .and_then(|images| images.get(idx))
                                        .and_then(|entry| entry.as_ref())
                                    else {
                                        return -12;
                                    };
                                    current_target_image = Some(img.image);
                                    current_vp_w = img.width.max(1);
                                    current_vp_h = img.height.max(1);
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
                        let (pipeline, vbuf, _) =
                            match ensure_gfx_resources_tex(ctx, tex_blob.len(), tex_kind) {
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
                                sample_kind,
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
                                    let mut st = GFX_CABI_STATE.lock();
                                    let idx = tex_id.saturating_sub(1) as usize;
                                    let should_log = st.missing_tex_logs < 16
                                        && st.last_missing_tex_id != tex_id;
                                    if should_log {
                                        st.last_missing_tex_id = tex_id;
                                        st.missing_tex_logs = st.missing_tex_logs.saturating_add(1);
                                    }
                                    let desc = ImageDesc {
                                        width: 1,
                                        height: 1,
                                        format: ImageFormat::Rgba8888,
                                    };
                                    let Ok(img) = ctx.create_image(desc) else {
                                        return -10;
                                    };
                                    let white = [255u8, 255u8, 255u8, 255u8];
                                    let _ = ctx.write_image(img, &white);
                                    let images = st.tex_images.get_or_insert_with(Vec::new);
                                    if idx >= images.len() {
                                        images.resize_with(idx + 1, || None);
                                    }
                                    images[idx] = Some(TexImage {
                                        image: img,
                                        width: 1,
                                        height: 1,
                                        sample_kind,
                                        origin: TexCoordOrigin::TopLeft,
                                    });
                                    (img, should_log)
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
                        let mut st = GFX_CABI_STATE.lock();
                        st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
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
                        let mut st = GFX_CABI_STATE.lock();
                        st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                        return 0;
                    }
                    if let Err(e) = submit_res {
                        crate::globalog::log(format_args!("gfx-cabi: submit failed: {:?}\n", e));
                    }
                    return -11;
                }
                0
            },
        ) else {
            return -13;
        };

        if ret == 0 {
            let mut st = GFX_CABI_STATE.lock();
            if is_screen_present_frame {
                st.base_cache_valid = true;
                st.base_cache_updated_at_ticks = embassy_time_driver::now();
                st.base_cache_clear_rgb = clear_rgb;
                st.base_cache_draws = draws.clone();
                st.base_cache_rgb_blob = rgb_src.clone();
                st.base_cache_tex_blob = tex_src.clone();
            }

            if crate::gfx::is_virgl_active() {
                let first =
                    !VIRGL_FIRST_FRAME_SEEN.swap(true, core::sync::atomic::Ordering::AcqRel);
                if first {
                    crate::v::readiness::set(crate::v::readiness::GFX_VIRGL_READY);
                    crate::globalog::log(format_args!(
                        "gfx: virgl first frame ready seq={} bytes={}\n",
                        seq, draw_bytes
                    ));
                }
                let n =
                    VIRGL_END_FRAME_DIAG_LOGS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                if first || n < 12 {
                    crate::globalog::log(format_args!(
                        "gfx-cabi: virgl end_frame ok seq={} rgb={} tex={} bytes={} first={}\n",
                        seq, rgb_draws, tex_draws, draw_bytes, first as u8
                    ));
                }
            }
        } else if crate::gfx::is_virgl_active() {
            let n = VIRGL_END_FRAME_DIAG_LOGS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if n < 12 {
                crate::globalog::log(format_args!(
                    "gfx-cabi: virgl end_frame failed seq={} rgb={} tex={} bytes={} rc={}\n",
                    seq, rgb_draws, tex_draws, draw_bytes, ret
                ));
            }
        }

        if seq <= 10 || (seq % 20) == 0 {
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
        let Some(m) = usb::input::pop_mouse_event() else {
            return 0;
        };
        *out_buttons = m.buttons;
        *out_dx = m.dx;
        *out_dy = m.dy;
        *out_wheel = m.wheel;
        1
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_input_keyboard_count() -> u32 {
        crate::v::keyboard::keyboard_count()
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_keyboard_modifiers(
        keyboard_id: u32,
        out_modifiers: *mut u8,
    ) -> i32 {
        if out_modifiers.is_null() {
            return -1;
        }
        let Some(modifiers) = crate::v::keyboard::keyboard_modifiers(keyboard_id) else {
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
        let Some(state) = crate::v::keyboard::keyboard_state(keyboard_id) else {
            return 1;
        };
        core::ptr::copy_nonoverlapping(state.keys.as_ptr(), out_keys, state.keys.len());
        core::ptr::copy_nonoverlapping(state.ascii.as_ptr(), out_ascii, state.ascii.len());
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_pop_keyboard_output(
        out: *mut crate::v::keyboard::TrueosKeyboardOutputEvent,
    ) -> i32 {
        if out.is_null() {
            return -1;
        }
        let Some(evt) = crate::v::keyboard::pop_output_event() else {
            return 0;
        };
        *out = evt;
        1
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_read_keyboard_output_since(
        read_seq: u64,
        out: *mut crate::v::keyboard::TrueosKeyboardOutputEvent,
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
                crate::v::keyboard::read_output_events_since(read_seq, out_slice);
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
        crate::v::keyboard::inject_text(slot_id, text, flags) as i32
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
        if crate::v::keyboard::inject_key(slot_id, codepoint, key_code, modifiers, flags) {
            1
        } else {
            0
        }
    }
}
