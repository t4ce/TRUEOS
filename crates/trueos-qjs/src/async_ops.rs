#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

use crate as qjs;

use crate::async_fs;
use crate::trueos_shims::{trueos_cabi_fs_read_file, trueos_cabi_fs_remove};

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_gfx_upload_texture_png_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_upload_texture_jpeg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_upload_texture_svg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32;
    fn trueos_cabi_gfx_texture_dimensions(
        tex_id: u32,
        out_width: *mut u32,
        out_height: *mut u32,
    ) -> i32;
}

const ASYNC_TEX_STATUS_UNKNOWN: i32 = 0;
const ASYNC_TEX_STATUS_PENDING: i32 = 1;
const ASYNC_TEX_STATUS_READY: i32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpKind {
    ReadText,
    ReadBytes,
    WriteText,
    /// Kernel net fetch (URL -> file) then read file and resolve with text.
    NetFetchText,
    /// Kernel net fetch (URL -> bytes) and resolve with bytes.
    NetFetchBytes,
}

#[derive(Clone)]
struct PendingOp {
    ctx_owner: *mut qjs::JSContext,
    op_id: u32,
    kind: OpKind,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
    /// Optional auxiliary payload owned by this async op.
    /// For NetFetchText this is the destination file path (UTF-8 bytes).
    aux: Vec<u8>,
}

// NOTE: This satisfies `static` requirements for `spin::Mutex`.
// QuickJS execution is expected to remain within a single owning task.
unsafe impl Send for PendingOp {}

static PENDING: Mutex<Vec<PendingOp>> = Mutex::new(Vec::new());

#[derive(Clone, Copy)]
struct CompletedOp {
    ctx_owner: *mut qjs::JSContext,
    op_id: u32,
}

// QuickJS execution is expected to remain within a single owning task.
unsafe impl Send for CompletedOp {}

static COMPLETED: Mutex<Vec<CompletedOp>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageRequestSource {
    LocalPath,
    RemoteUrl,
}

enum PendingImageStage {
    SourceBytes {
        op_id: u32,
        path: Vec<u8>,
        source: ImageRequestSource,
    },
    Upload,
}

struct PendingImageOp {
    ctx_owner: *mut qjs::JSContext,
    stage: PendingImageStage,
    tex_id: u32,
    width: u32,
    height: u32,
    mime: String,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
}

unsafe impl Send for PendingImageOp {}

static PENDING_IMAGES: Mutex<Vec<PendingImageOp>> = Mutex::new(Vec::new());
static IMAGE_DIAG_LOGS: AtomicU32 = AtomicU32::new(0);

#[inline]
fn image_diag_log(msg: &str) {
    if msg.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(2, msg.as_ptr(), msg.len()) };
}

fn image_diag_allowed() -> bool {
    IMAGE_DIAG_LOGS.fetch_add(1, Ordering::Relaxed) < 64
}

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

pub unsafe fn start_read_file(path: &[u8]) -> Result<u32, i32> {
    async_fs::start_read_file(path)
}

pub unsafe fn start_write_file(path: &[u8], data: &[u8]) -> Result<u32, i32> {
    async_fs::start_write_file(path, data)
}

pub unsafe fn start_net_fetch_to_file(url: &[u8], path: &[u8]) -> Result<u32, i32> {
    async_fs::start_net_fetch_to_file(url, path)
}

pub unsafe fn start_net_fetch_bytes(url: &[u8]) -> Result<u32, i32> {
    async_fs::start_net_fetch_bytes(url)
}

pub unsafe fn start_net_post_json_to_file(
    url: &[u8],
    path: &[u8],
    body_json: &[u8],
    bearer: Option<&[u8]>,
) -> Result<u32, i32> {
    async_fs::start_net_post_json_to_file(url, path, body_json, bearer)
}

pub unsafe fn register_promise(
    ctx: *mut qjs::JSContext,
    op_id: u32,
    kind: OpKind,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
    aux: Vec<u8>,
) {
    let op = PendingOp {
        ctx_owner: ctx,
        op_id,
        kind,
        resolve: unsafe { qjs::js_dup_value(ctx, resolve) },
        reject: unsafe { qjs::js_dup_value(ctx, reject) },
        aux,
    };
    PENDING.lock().push(op);
}

pub unsafe fn register_ready_image_texture_request(
    ctx: *mut qjs::JSContext,
    op_id: u32,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
    path: Vec<u8>,
    tex_id: u32,
    source: ImageRequestSource,
) {
    if image_diag_allowed() {
        image_diag_log(&format!(
            "img-async: register tex_id={} source={:?} path_len={} op_id={}\n",
            tex_id,
            source,
            path.len(),
            op_id
        ));
    }
    let op = PendingImageOp {
        ctx_owner: ctx,
        stage: PendingImageStage::SourceBytes {
            op_id,
            path,
            source,
        },
        tex_id,
        width: 0,
        height: 0,
        mime: String::new(),
        resolve: qjs::js_dup_value(ctx, resolve),
        reject: qjs::js_dup_value(ctx, reject),
    };
    PENDING_IMAGES.lock().push(op);
}

pub unsafe fn has_pending(ctx: *mut qjs::JSContext) -> bool {
    PENDING.lock().iter().any(|p| p.ctx_owner == ctx)
        || PENDING_IMAGES.lock().iter().any(|p| p.ctx_owner == ctx)
}

unsafe fn take_pending(ctx: *mut qjs::JSContext, op_id: u32) -> Option<PendingOp> {
    let mut pending = PENDING.lock();
    let pos = pending
        .iter()
        .position(|p| p.ctx_owner == ctx && p.op_id == op_id)?;
    Some(pending.remove(pos))
}

fn find_pending_owner(op_id: u32) -> Option<*mut qjs::JSContext> {
    PENDING
        .lock()
        .iter()
        .find(|p| p.op_id == op_id)
        .map(|p| p.ctx_owner)
}

fn find_pending_image_owner(op_id: u32) -> Option<*mut qjs::JSContext> {
    PENDING_IMAGES.lock().iter().find_map(|p| match &p.stage {
        PendingImageStage::SourceBytes {
            op_id: pending_id, ..
        } if *pending_id == op_id => Some(p.ctx_owner),
        _ => None,
    })
}

fn push_completed(ctx: *mut qjs::JSContext, op_id: u32) {
    COMPLETED.lock().push(CompletedOp {
        ctx_owner: ctx,
        op_id,
    });
}

fn take_completed_for_ctx(ctx: *mut qjs::JSContext) -> Option<u32> {
    let mut completed = COMPLETED.lock();
    let pos = completed.iter().position(|c| c.ctx_owner == ctx)?;
    Some(completed.remove(pos).op_id)
}

unsafe fn reject_with_code(ctx: *mut qjs::JSContext, op: &PendingOp, code: i32) {
    let arg = js_int32(code);
    let _ = qjs::JS_Call(
        ctx,
        op.reject,
        qjs::JSValue::undefined(),
        1,
        &arg as *const qjs::JSValue,
    );
}

unsafe fn resolve_with_value(ctx: *mut qjs::JSContext, op: &PendingOp, val: qjs::JSValue) {
    let _ = qjs::JS_Call(
        ctx,
        op.resolve,
        qjs::JSValue::undefined(),
        1,
        &val as *const qjs::JSValue,
    );
    qjs::js_free_value(ctx, val);
}

unsafe fn resolve_undefined(ctx: *mut qjs::JSContext, op: &PendingOp) {
    let val = qjs::JSValue::undefined();
    let _ = qjs::JS_Call(
        ctx,
        op.resolve,
        qjs::JSValue::undefined(),
        1,
        &val as *const qjs::JSValue,
    );
}

fn read_file_via_cabi(path: &[u8]) -> Result<Vec<u8>, i32> {
    let len =
        unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    let len = len as usize;
    let mut buf: Vec<u8> = Vec::with_capacity(len);
    buf.resize(len, 0);
    let got =
        unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), buf.as_mut_ptr(), buf.len()) };
    if got < 0 {
        return Err(got as i32);
    }
    buf.truncate(got as usize);
    Ok(buf)
}

fn read_png_dimensions(bytes: &[u8]) -> Result<(u32, u32), i32> {
    if bytes.len() < 24 {
        return Err(-7);
    }
    if bytes[0..8] != [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return Err(-7);
    }
    if &bytes[12..16] != b"IHDR" {
        return Err(-7);
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if width == 0 || height == 0 {
        return Err(-7);
    }
    Ok((width, height))
}

fn queue_png_texture_upload(tex_id: u32, bytes: &[u8]) -> Result<(u32, u32), i32> {
    let (width, height) = read_png_dimensions(bytes)?;
    let rc =
        unsafe { trueos_cabi_gfx_upload_texture_png_async(tex_id, bytes.as_ptr(), bytes.len()) };
    if rc != 0 {
        return Err(rc);
    }
    Ok((width, height))
}

fn jpeg_like_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

fn path_has_jpeg_suffix(path: &[u8]) -> bool {
    let text = core::str::from_utf8(path).unwrap_or("");
    let lower = text.as_bytes();
    lower.ends_with(b".jpg") || lower.ends_with(b".jpeg")
}

fn queue_jpeg_texture_upload(tex_id: u32, bytes: &[u8]) -> Result<(), i32> {
    let rc =
        unsafe { trueos_cabi_gfx_upload_texture_jpeg_async(tex_id, bytes.as_ptr(), bytes.len()) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

fn svg_like_bytes(bytes: &[u8]) -> bool {
    let mut start = 0usize;
    while start < bytes.len() && matches!(bytes[start], b' ' | b'\t' | b'\r' | b'\n') {
        start += 1;
    }
    let trimmed = &bytes[start..];
    trimmed.starts_with(b"<svg")
        || trimmed.starts_with(b"<?xml")
        || trimmed.windows(4).any(|window| window == b"<svg")
}

fn path_has_svg_suffix(path: &[u8]) -> bool {
    let text = core::str::from_utf8(path).unwrap_or("");
    let lower = text.as_bytes();
    lower.ends_with(b".svg") || lower.ends_with(b".svgz")
}

fn queue_svg_texture_upload(tex_id: u32, bytes: &[u8]) -> Result<(), i32> {
    let rc =
        unsafe { trueos_cabi_gfx_upload_texture_svg_async(tex_id, bytes.as_ptr(), bytes.len()) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

fn texture_dimensions(tex_id: u32) -> Option<(u32, u32)> {
    let mut width = 0u32;
    let mut height = 0u32;
    let rc = unsafe {
        trueos_cabi_gfx_texture_dimensions(tex_id, &mut width as *mut u32, &mut height as *mut u32)
    };
    if rc == 0 && width > 0 && height > 0 {
        Some((width, height))
    } else {
        None
    }
}

unsafe fn resolve_image_op(ctx: *mut qjs::JSContext, op: &PendingImageOp) {
    let obj = qjs::JS_NewObject(ctx);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"texId\0".as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, op.tex_id as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"width\0".as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, op.width as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"height\0".as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, op.height as f64),
    );
    let mime = qjs::JS_NewStringLen(ctx, op.mime.as_ptr() as *const c_char, op.mime.len());
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"mime\0".as_ptr() as *const c_char, mime);
    let _ = qjs::JS_Call(
        ctx,
        op.resolve,
        qjs::JSValue::undefined(),
        1,
        &obj as *const qjs::JSValue,
    );
    qjs::js_free_value(ctx, obj);
}

unsafe fn reject_image_op(ctx: *mut qjs::JSContext, op: &PendingImageOp, code: i32) {
    let arg = js_int32(code);
    let _ = qjs::JS_Call(
        ctx,
        op.reject,
        qjs::JSValue::undefined(),
        1,
        &arg as *const qjs::JSValue,
    );
}

unsafe fn pump_image_requests(ctx: *mut qjs::JSContext) -> bool {
    let mut progress = false;
    let mut ops: Vec<PendingImageOp> = Vec::new();
    {
        let mut pending = PENDING_IMAGES.lock();
        let mut idx = 0usize;
        while idx < pending.len() {
            if pending[idx].ctx_owner == ctx {
                ops.push(pending.remove(idx));
            } else {
                idx += 1;
            }
        }
    }

    for mut op in ops {
        let mut keep = false;
        match &mut op.stage {
            PendingImageStage::SourceBytes {
                op_id,
                path,
                source,
            } => {
                let rc_or_done = async_fs::result_len(*op_id);
                if rc_or_done == async_fs::FS_ERR_NOT_FOUND as isize {
                    keep = true;
                } else if rc_or_done < 0 {
                    progress = true;
                    if image_diag_allowed() {
                        image_diag_log(&format!(
                            "img-async: bytes-op-error tex_id={} source={:?} op_id={} code={}\n",
                            op.tex_id, source, *op_id, rc_or_done
                        ));
                    }
                    let _ = async_fs::discard(*op_id);
                    reject_image_op(ctx, &op, rc_or_done as i32);
                    crate::cmd_stream::release_managed_tex_id(op.tex_id);
                } else {
                    progress = true;
                    match source {
                        ImageRequestSource::RemoteUrl => {
                            if image_diag_allowed() {
                                image_diag_log(&format!(
                                    "img-async: fetch-done tex_id={} op_id={} len={}\n",
                                    op.tex_id, *op_id, rc_or_done
                                ));
                            }
                        }
                        ImageRequestSource::LocalPath => {
                            if image_diag_allowed() {
                                image_diag_log(&format!(
                                    "img-async: bytes-op-done tex_id={} source=LocalPath op_id={} len={}\n",
                                    op.tex_id, *op_id, rc_or_done
                                ));
                            }
                        }
                    }
                    match read_completion_bytes(*op_id, rc_or_done as usize) {
                        Ok(bytes) => {
                            let is_svg = path_has_svg_suffix(path.as_slice())
                                || svg_like_bytes(bytes.as_slice());
                            let is_jpeg = !is_svg
                                && (path_has_jpeg_suffix(path.as_slice())
                                    || jpeg_like_bytes(bytes.as_slice()));
                            let queued = if is_svg {
                                queue_svg_texture_upload(op.tex_id, bytes.as_slice()).map(|_| {
                                    op.mime = String::from("image/svg+xml");
                                    op.width = 0;
                                    op.height = 0;
                                })
                            } else if is_jpeg {
                                queue_jpeg_texture_upload(op.tex_id, bytes.as_slice()).map(|_| {
                                    op.mime = String::from("image/jpeg");
                                    op.width = 0;
                                    op.height = 0;
                                })
                            } else {
                                queue_png_texture_upload(op.tex_id, bytes.as_slice()).map(
                                    |(width, height)| {
                                        op.mime = String::from("image/png");
                                        op.width = width;
                                        op.height = height;
                                    },
                                )
                            };
                            match queued {
                                Ok(()) => {
                                    if image_diag_allowed() {
                                        image_diag_log(&format!(
                                            "img-async: upload-queued tex_id={} mime={}\n",
                                            op.tex_id,
                                            op.mime.as_str()
                                        ));
                                    }
                                    op.stage = PendingImageStage::Upload;
                                    keep = true;
                                }
                                Err(code) => {
                                    if image_diag_allowed() {
                                        image_diag_log(&format!(
                                            "img-async: upload-queue-error tex_id={} code={}\n",
                                            op.tex_id, code
                                        ));
                                    }
                                    reject_image_op(ctx, &op, code);
                                    crate::cmd_stream::release_managed_tex_id(op.tex_id);
                                }
                            }
                        }
                        Err(code) => {
                            if image_diag_allowed() {
                                image_diag_log(&format!(
                                    "img-async: read-bytes-error tex_id={} code={}\n",
                                    op.tex_id, code
                                ));
                            }
                            reject_image_op(ctx, &op, code);
                            crate::cmd_stream::release_managed_tex_id(op.tex_id);
                        }
                    }
                }
            }
            PendingImageStage::Upload => {
                let status = trueos_cabi_gfx_texture_status(op.tex_id);
                if status == ASYNC_TEX_STATUS_READY {
                    progress = true;
                    if (op.width == 0 || op.height == 0)
                        && let Some((width, height)) = texture_dimensions(op.tex_id)
                    {
                        op.width = width;
                        op.height = height;
                    }
                    if image_diag_allowed() {
                        image_diag_log(&format!(
                            "img-async: ready tex_id={} {}x{} mime={}\n",
                            op.tex_id,
                            op.width,
                            op.height,
                            op.mime.as_str()
                        ));
                    }
                    resolve_image_op(ctx, &op);
                } else if status < 0 {
                    progress = true;
                    if image_diag_allowed() {
                        image_diag_log(&format!(
                            "img-async: upload-status-error tex_id={} code={}\n",
                            op.tex_id, status
                        ));
                    }
                    reject_image_op(ctx, &op, status);
                    crate::cmd_stream::release_managed_tex_id(op.tex_id);
                } else {
                    keep = status == ASYNC_TEX_STATUS_PENDING || status == ASYNC_TEX_STATUS_UNKNOWN;
                }
            }
        }

        if keep {
            PENDING_IMAGES.lock().push(op);
        } else {
            qjs::js_free_value(ctx, op.resolve);
            qjs::js_free_value(ctx, op.reject);
        }
    }

    progress
}

unsafe fn pump_net_fetch_text(ctx: *mut qjs::JSContext) -> bool {
    // Net ops don't show up in async_fs::poll_completed(), so we must poll their result here.
    let mut progress = false;

    // Collect op_ids to process to avoid holding the mutex while doing work.
    let op_ids: Vec<u32> = {
        let pending = PENDING.lock();
        pending
            .iter()
            .filter(|p| p.ctx_owner == ctx && p.kind == OpKind::NetFetchText)
            .map(|p| p.op_id)
            .collect()
    };

    for op_id in op_ids {
        // Still in flight?
        let rc_or_done = async_fs::result_len(op_id);
        if rc_or_done == async_fs::FS_ERR_NOT_FOUND as isize {
            continue;
        }

        let Some(mut op) = take_pending(ctx, op_id) else {
            let _ = async_fs::discard(op_id);
            continue;
        };

        progress = true;

        if rc_or_done < 0 {
            let _ = async_fs::discard(op_id);
            reject_with_code(ctx, &op, rc_or_done as i32);
            qjs::js_free_value(ctx, op.resolve);
            qjs::js_free_value(ctx, op.reject);
            continue;
        }

        // Consume the net op result (also discards the kernel op id), then schedule
        // a separate async file read so the QJS owner task never blocks on temp-file I/O.
        let _ = async_fs::read_result(op_id, core::ptr::null_mut(), 0);

        match async_fs::start_read_file(op.aux.as_slice()) {
            Ok(read_op_id) => {
                op.op_id = read_op_id;
                op.kind = OpKind::ReadText;
                PENDING.lock().push(op);
                continue;
            }
            Err(code) => reject_with_code(ctx, &op, code),
        }

        qjs::js_free_value(ctx, op.resolve);
        qjs::js_free_value(ctx, op.reject);
    }

    progress
}

unsafe fn pump_net_fetch_bytes(ctx: *mut qjs::JSContext) -> bool {
    let mut progress = false;

    let op_ids: Vec<u32> = {
        let pending = PENDING.lock();
        pending
            .iter()
            .filter(|p| p.ctx_owner == ctx && p.kind == OpKind::NetFetchBytes)
            .map(|p| p.op_id)
            .collect()
    };

    for op_id in op_ids {
        let rc_or_done = async_fs::result_len(op_id);
        if rc_or_done == async_fs::FS_ERR_NOT_FOUND as isize {
            continue;
        }

        let Some( op) = take_pending(ctx, op_id) else {
            let _ = async_fs::discard(op_id);
            continue;
        };

        progress = true;

        if rc_or_done < 0 {
            let _ = async_fs::discard(op_id);
            reject_with_code(ctx, &op, rc_or_done as i32);
            qjs::js_free_value(ctx, op.resolve);
            qjs::js_free_value(ctx, op.reject);
            continue;
        }

        let n = rc_or_done as usize;
        match read_completion_bytes(op_id, n) {
            Ok(buf) => {
                let ab = qjs::JS_NewArrayBufferCopy(ctx, buf.as_ptr(), buf.len());
                resolve_with_value(ctx, &op, ab);
            }
            Err(code) => reject_with_code(ctx, &op, code),
        }

        qjs::js_free_value(ctx, op.resolve);
        qjs::js_free_value(ctx, op.reject);
    }

    progress
}

fn read_completion_bytes(done_id: u32, len: usize) -> Result<Vec<u8>, i32> {
    let mut buf: Vec<u8> = Vec::with_capacity(len);
    buf.resize(len, 0);
    let got = async_fs::read_result(done_id, buf.as_mut_ptr(), buf.len());
    if got < 0 {
        return Err(got as i32);
    }
    buf.truncate(got as usize);
    Ok(buf)
}

/// Pump completed kernel async FS operations into JS Promises.
///
/// Call this from the thread/task that owns the QuickJS runtime.
///
/// Returns `true` if it made progress (handled at least one completion).
pub unsafe fn pump(ctx: *mut qjs::JSContext) -> bool {
    if ctx.is_null() {
        return false;
    }

    let mut progress = false;

    progress |= pump_image_requests(ctx);
    progress |= pump_net_fetch_text(ctx);
    progress |= pump_net_fetch_bytes(ctx);

    loop {
        let done_id = match take_completed_for_ctx(ctx) {
            Some(id) => {
                progress = true;
                id
            }
            None => {
                let mut id: u32 = 0;
                let has = async_fs::poll_completed(&mut id as *mut u32);
                if has <= 0 {
                    break;
                }
                progress = true;

                let owner = find_pending_owner(id);
                if let Some(owner) = owner {
                    if owner != ctx {
                        push_completed(owner, id);
                        continue;
                    }
                } else {
                    if find_pending_image_owner(id).is_some() {
                        continue;
                    }
                    let _ = async_fs::discard(id);
                    continue;
                }
                id
            }
        };

        let Some(op) = (unsafe { take_pending(ctx, done_id) }) else {
            // Unknown/expired op id: drop completion to avoid leaks.
            let _ = async_fs::discard(done_id);
            continue;
        };

        let len = async_fs::result_len(done_id);
        if len < 0 {
            unsafe { reject_with_code(ctx, &op, len as i32) };
            unsafe { qjs::js_free_value(ctx, op.resolve) };
            unsafe { qjs::js_free_value(ctx, op.reject) };
            continue;
        }

        match op.kind {
            OpKind::WriteText => {
                // Consume completion payload (if any) and resolve.
                let _ = async_fs::read_result(done_id, core::ptr::null_mut(), 0);
                unsafe { resolve_undefined(ctx, &op) };
            }
            OpKind::ReadBytes => {
                let n = len as usize;
                match read_completion_bytes(done_id, n) {
                    Ok(buf) => {
                        let ab =
                            unsafe { qjs::JS_NewArrayBufferCopy(ctx, buf.as_ptr(), buf.len()) };
                        unsafe { resolve_with_value(ctx, &op, ab) };
                    }
                    Err(code) => unsafe { reject_with_code(ctx, &op, code) },
                }
            }
            OpKind::ReadText => {
                let n = len as usize;
                match read_completion_bytes(done_id, n) {
                    Ok(buf) => {
                        let s = unsafe {
                            qjs::JS_NewStringLen(
                                ctx,
                                buf.as_ptr() as *const core::ffi::c_char,
                                buf.len(),
                            )
                        };
                        unsafe { resolve_with_value(ctx, &op, s) };
                    }
                    Err(code) => unsafe { reject_with_code(ctx, &op, code) },
                }
                if !op.aux.is_empty() {
                    let _ = unsafe { trueos_cabi_fs_remove(op.aux.as_ptr(), op.aux.len()) };
                }
            }
            OpKind::NetFetchText => {
                // Net fetches are handled by `pump_net_fetch_text`.
                // If one ever arrives here, discard and reject to avoid leaking.
                let _ = async_fs::discard(done_id);
                reject_with_code(ctx, &op, async_fs::FS_ERR_BAD_PARAM);
            }
            OpKind::NetFetchBytes => {
                // Net fetches are handled by `pump_net_fetch_bytes`.
                // If one ever arrives here, discard and reject to avoid leaking.
                let _ = async_fs::discard(done_id);
                reject_with_code(ctx, &op, async_fs::FS_ERR_BAD_PARAM);
            }
        }

        unsafe { qjs::js_free_value(ctx, op.resolve) };
        unsafe { qjs::js_free_value(ctx, op.reject) };
    }

    progress
}

/// Create a Promise (and its resolve/reject functions).
///
/// Returns (promise, resolve, reject).
pub unsafe fn new_promise(ctx: *mut qjs::JSContext) -> (qjs::JSValue, qjs::JSValue, qjs::JSValue) {
    let mut resolving: [qjs::JSValue; 2] = [qjs::JSValue::undefined(), qjs::JSValue::undefined()];
    let promise = unsafe { qjs::JS_NewPromiseCapability(ctx, resolving.as_mut_ptr()) };
    (promise, resolving[0], resolving[1])
}

pub unsafe fn drain_all_for_context(ctx: *mut qjs::JSContext) {
    // Best-effort cleanup: reject all pending promises for this context.
    let mut pending = PENDING.lock();
    let mut i = 0usize;
    while i < pending.len() {
        if pending[i].ctx_owner != ctx {
            i += 1;
            continue;
        }
        let op = pending.remove(i);
        reject_with_code(ctx, &op, -2);
        qjs::js_free_value(ctx, op.resolve);
        qjs::js_free_value(ctx, op.reject);
        let _ = async_fs::discard(op.op_id);
    }

    let mut pending_images = PENDING_IMAGES.lock();
    let mut image_idx = 0usize;
    while image_idx < pending_images.len() {
        if pending_images[image_idx].ctx_owner != ctx {
            image_idx += 1;
            continue;
        }
        let op = pending_images.remove(image_idx);
        reject_image_op(ctx, &op, -2);
        qjs::js_free_value(ctx, op.resolve);
        qjs::js_free_value(ctx, op.reject);
        match op.stage {
            PendingImageStage::SourceBytes { op_id, path, .. } => {
                let _ = async_fs::discard(op_id);
                if !path.is_empty() {
                    let _ = unsafe { trueos_cabi_fs_remove(path.as_ptr(), path.len()) };
                }
            }
            PendingImageStage::Upload => {}
        }
        crate::cmd_stream::release_managed_tex_id(op.tex_id);
    }

    let mut discard_ids: Vec<u32> = Vec::new();
    {
        let mut completed = COMPLETED.lock();
        let mut j = 0usize;
        while j < completed.len() {
            if completed[j].ctx_owner == ctx {
                discard_ids.push(completed.remove(j).op_id);
            } else {
                j += 1;
            }
        }
    }
    for op_id in discard_ids {
        let _ = async_fs::discard(op_id);
    }
}

pub async fn wait_for_completion(timeout_ms: u64) -> bool {
    async_fs::wait_for_completion(timeout_ms).await
}
