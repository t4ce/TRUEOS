#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ffi::c_char;

use spin::Mutex;

use crate as qjs;

use crate::async_fs;
use crate::trueos_shims::{trueos_cabi_fs_read_file, trueos_cabi_fs_remove};

unsafe extern "C" {
    fn trueos_cabi_gfx_upload_texture_png_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32;
}

const ASYNC_TEX_STATUS_PENDING: i32 = 1;
const ASYNC_TEX_STATUS_READY: i32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpKind {
    ReadText,
    ReadBytes,
    WriteText,
    /// Kernel net fetch (URL -> file) then read file and resolve with text.
    NetFetchText,
    /// Kernel net fetch (URL -> file) then read file and resolve with bytes.
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
pub enum ImageBytesSource {
    ReadFile,
    NetFile,
}

enum PendingImageStage {
    BytesOp {
        op_id: u32,
        path: Vec<u8>,
        source: ImageBytesSource,
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

pub unsafe fn register_image_fetch_promise(
    ctx: *mut qjs::JSContext,
    op_id: u32,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
    path: Vec<u8>,
    tex_id: u32,
    source: ImageBytesSource,
) {
    let op = PendingImageOp {
        ctx_owner: ctx,
        stage: PendingImageStage::BytesOp {
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

pub unsafe fn start_data_url_image_promise(
    ctx: *mut qjs::JSContext,
    url: &[u8],
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
    tex_id: u32,
) -> Result<(), i32> {
    let url_str = core::str::from_utf8(url).map_err(|_| async_fs::FS_ERR_BAD_UTF8)?;
    let (mime, bytes) = decode_data_url(url_str)?;
    let (width, height) = queue_png_texture_upload(tex_id, bytes.as_slice())?;
    let op = PendingImageOp {
        ctx_owner: ctx,
        stage: PendingImageStage::Upload,
        tex_id,
        width,
        height,
        mime,
        resolve: qjs::js_dup_value(ctx, resolve),
        reject: qjs::js_dup_value(ctx, reject),
    };
    PENDING_IMAGES.lock().push(op);
    Ok(())
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

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_bytes(input: &str) -> Result<Vec<u8>, i32> {
    let src = input.as_bytes();
    let mut out = Vec::with_capacity(src.len());
    let mut idx = 0usize;
    while idx < src.len() {
        let byte = src[idx];
        if byte == b'%' {
            if idx + 2 >= src.len() {
                return Err(async_fs::FS_ERR_BAD_PARAM);
            }
            let hi = decode_hex_nibble(src[idx + 1]).ok_or(async_fs::FS_ERR_BAD_PARAM)?;
            let lo = decode_hex_nibble(src[idx + 2]).ok_or(async_fs::FS_ERR_BAD_PARAM)?;
            out.push((hi << 4) | lo);
            idx += 3;
            continue;
        }
        out.push(byte);
        idx += 1;
    }
    Ok(out)
}

fn decode_base64_payload(input: &str) -> Result<Vec<u8>, i32> {
    fn val(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity((bytes.len() / 4).saturating_mul(3));
    let mut chunk = [0u8; 4];
    let mut chunk_len = 0usize;

    for &byte in bytes {
        if matches!(byte, b'\r' | b'\n' | b'\t' | b' ') {
            continue;
        }
        chunk[chunk_len] = byte;
        chunk_len += 1;
        if chunk_len < 4 {
            continue;
        }

        let a = val(chunk[0]).ok_or(async_fs::FS_ERR_BAD_PARAM)?;
        let b = val(chunk[1]).ok_or(async_fs::FS_ERR_BAD_PARAM)?;
        let c = if chunk[2] == b'=' {
            64
        } else {
            val(chunk[2]).ok_or(async_fs::FS_ERR_BAD_PARAM)?
        };
        let d = if chunk[3] == b'=' {
            64
        } else {
            val(chunk[3]).ok_or(async_fs::FS_ERR_BAD_PARAM)?
        };

        out.push((a << 2) | (b >> 4));
        if c != 64 {
            out.push(((b & 0x0F) << 4) | (c >> 2));
        }
        if d != 64 && c != 64 {
            out.push(((c & 0x03) << 6) | d);
        }

        chunk_len = 0;
    }

    if chunk_len != 0 {
        return Err(async_fs::FS_ERR_BAD_PARAM);
    }

    Ok(out)
}

fn decode_data_url(url: &str) -> Result<(String, Vec<u8>), i32> {
    let Some(rest) = url.strip_prefix("data:") else {
        return Err(async_fs::FS_ERR_BAD_PARAM);
    };
    let Some((meta, payload)) = rest.split_once(',') else {
        return Err(async_fs::FS_ERR_BAD_PARAM);
    };

    let mut mime = String::from("text/plain");
    let mut is_base64 = false;
    for (idx, part) in meta.split(';').enumerate() {
        let item = part.trim();
        if item.is_empty() {
            continue;
        }
        if idx == 0 && !item.contains('=') {
            mime = item.to_ascii_lowercase();
            continue;
        }
        if item.eq_ignore_ascii_case("base64") {
            is_base64 = true;
        }
    }

    let bytes = if is_base64 {
        decode_base64_payload(payload)?
    } else {
        percent_decode_bytes(payload)?
    };
    Ok((mime, bytes))
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
    let rc = unsafe { trueos_cabi_gfx_upload_texture_png_async(tex_id, bytes.as_ptr(), bytes.len()) };
    if rc != 0 {
        return Err(rc);
    }
    Ok((width, height))
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

fn read_fs_completion_bytes(op_id: u32, len: usize) -> Result<Vec<u8>, i32> {
    let mut buf = Vec::with_capacity(len);
    buf.resize(len, 0);
    let got = async_fs::read_result(op_id, buf.as_mut_ptr(), buf.len());
    if got < 0 {
        return Err(got as i32);
    }
    buf.truncate(got as usize);
    Ok(buf)
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
            PendingImageStage::BytesOp {
                op_id,
                path,
                source,
            } => {
                let rc_or_done = async_fs::result_len(*op_id);
                if rc_or_done == async_fs::FS_ERR_NOT_FOUND as isize {
                    keep = true;
                } else if rc_or_done < 0 {
                    progress = true;
                    let _ = async_fs::discard(*op_id);
                    reject_image_op(ctx, &op, rc_or_done as i32);
                    crate::cmd_stream::release_managed_tex_id(op.tex_id);
                } else {
                    progress = true;
                    let bytes_res = match source {
                        ImageBytesSource::ReadFile => read_fs_completion_bytes(*op_id, rc_or_done as usize),
                        ImageBytesSource::NetFile => {
                            let _ = async_fs::read_result(*op_id, core::ptr::null_mut(), 0);
                            let out = read_file_via_cabi(path.as_slice());
                            let _ = trueos_cabi_fs_remove(path.as_ptr(), path.len());
                            out
                        }
                    };

                    match bytes_res.and_then(|bytes| queue_png_texture_upload(op.tex_id, bytes.as_slice())) {
                        Ok((width, height)) => {
                            op.width = width;
                            op.height = height;
                            op.mime = String::from("image/png");
                            op.stage = PendingImageStage::Upload;
                            keep = true;
                        }
                        Err(code) => {
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
                    resolve_image_op(ctx, &op);
                } else if status < 0 {
                    progress = true;
                    reject_image_op(ctx, &op, status);
                    crate::cmd_stream::release_managed_tex_id(op.tex_id);
                } else {
                    keep = status == ASYNC_TEX_STATUS_PENDING || status == 0;
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

        let Some(op) = take_pending(ctx, op_id) else {
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

        // Consume the net op result (also discards the kernel op id).
        let _ = async_fs::read_result(op_id, core::ptr::null_mut(), 0);

        // Read downloaded bytes and resolve as a JS string.
        match read_file_via_cabi(op.aux.as_slice()) {
            Ok(buf) => {
                let s =
                    qjs::JS_NewStringLen(ctx, buf.as_ptr() as *const core::ffi::c_char, buf.len());
                resolve_with_value(ctx, &op, s);
            }
            Err(code) => {
                reject_with_code(ctx, &op, code);
            }
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

        let Some(op) = take_pending(ctx, op_id) else {
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

        let _ = async_fs::read_result(op_id, core::ptr::null_mut(), 0);

        match read_file_via_cabi(op.aux.as_slice()) {
            Ok(buf) => {
                let ab = qjs::JS_NewArrayBufferCopy(ctx, buf.as_ptr(), buf.len());
                resolve_with_value(ctx, &op, ab);
            }
            Err(code) => {
                reject_with_code(ctx, &op, code);
            }
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
        if let PendingImageStage::BytesOp { op_id, source, path } = op.stage {
            let _ = async_fs::discard(op_id);
            if source == ImageBytesSource::NetFile {
                let _ = trueos_cabi_fs_remove(path.as_ptr(), path.len());
            }
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
