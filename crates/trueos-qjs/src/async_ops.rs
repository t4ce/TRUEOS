#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

use crate as qjs;

use crate::async_fs;
use crate::platform;
use crate::trueos_shims::{trueos_cabi_fs_read_file, trueos_cabi_fs_remove};

const ASYNC_TEX_STATUS_UNKNOWN: i32 = 0;
const ASYNC_TEX_STATUS_PENDING: i32 = 1;
const ASYNC_TEX_STATUS_READY: i32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpKind {
    ReadText,
    ReadBytes,
    WriteText,
    /// Kernel net fetch (URL -> file) then read file and resolve with text.
    NetFetchTextFile,
    /// Kernel net POST(JSON) -> bytes and resolve with text.
    NetPostJsonTextBytes,
    /// Kernel net fetch (URL -> bytes) and resolve with bytes.
    NetFetchBytes,
    /// Kernel net fetch (URL -> cache file) and resolve with the normalized specifier.
    NetFetchModule,
}

#[derive(Clone)]
struct PendingOp {
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

static PENDING: Mutex<BTreeMap<usize, Vec<PendingOp>>> = Mutex::new(BTreeMap::new());
static COMPLETED: Mutex<BTreeMap<usize, Vec<u32>>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageRequestSource {
    InlineData,
    LocalPath,
    RemoteUrl,
}

enum PendingImageStage {
    CachedReady {
        cached: ReadyImageCacheEntry,
    },
    InlineBytes {
        path: Vec<u8>,
        bytes: Vec<u8>,
        source: ImageRequestSource,
    },
    SourceBytes {
        op_id: u32,
        path: Vec<u8>,
        source: ImageRequestSource,
    },
    Upload,
}

struct PendingImageOp {
    stage: PendingImageStage,
    cache_key: Option<Vec<u8>>,
    tex_id: u32,
    width: u32,
    height: u32,
    mime: String,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
}

unsafe impl Send for PendingImageOp {}

static PENDING_IMAGES: Mutex<BTreeMap<usize, Vec<PendingImageOp>>> = Mutex::new(BTreeMap::new());
static READY_IMAGE_CACHE: Mutex<BTreeMap<usize, Vec<ReadyImageCacheEntry>>> =
    Mutex::new(BTreeMap::new());
static IMAGE_DIAG_LOGS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct ReadyImageCacheEntry {
    key: Vec<u8>,
    tex_id: u32,
    width: u32,
    height: u32,
    mime: String,
}

#[inline]
fn image_diag_log(msg: &str) {
    if msg.is_empty() {
        return;
    }
    platform::sys::write_stderr(msg.as_bytes());
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

pub unsafe fn start_net_post_json_bytes(
    url: &[u8],
    body_json: &[u8],
    bearer: Option<&[u8]>,
) -> Result<u32, i32> {
    async_fs::start_net_post_json_bytes(url, body_json, bearer)
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
        op_id,
        kind,
        resolve: unsafe { qjs::js_dup_value(ctx, resolve) },
        reject: unsafe { qjs::js_dup_value(ctx, reject) },
        aux,
    };
    PENDING.lock().entry(ctx as usize).or_default().push(op);
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
    let cache_key = match source {
        ImageRequestSource::InlineData => None,
        ImageRequestSource::LocalPath | ImageRequestSource::RemoteUrl => Some(path.clone()),
    };
    let op = PendingImageOp {
        stage: PendingImageStage::SourceBytes {
            op_id,
            path,
            source,
        },
        cache_key,
        tex_id,
        width: 0,
        height: 0,
        mime: String::new(),
        resolve: qjs::js_dup_value(ctx, resolve),
        reject: qjs::js_dup_value(ctx, reject),
    };
    PENDING_IMAGES
        .lock()
        .entry(ctx as usize)
        .or_default()
        .push(op);
}

pub unsafe fn register_ready_image_texture_bytes(
    ctx: *mut qjs::JSContext,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
    path: Vec<u8>,
    bytes: Vec<u8>,
    tex_id: u32,
    source: ImageRequestSource,
) {
    if image_diag_allowed() {
        image_diag_log(&format!(
            "img-async: register tex_id={} source={:?} path_len={} bytes={}\n",
            tex_id,
            source,
            path.len(),
            bytes.len()
        ));
    }
    let op = PendingImageOp {
        stage: PendingImageStage::InlineBytes {
            path,
            bytes,
            source,
        },
        cache_key: None,
        tex_id,
        width: 0,
        height: 0,
        mime: String::new(),
        resolve: qjs::js_dup_value(ctx, resolve),
        reject: qjs::js_dup_value(ctx, reject),
    };
    PENDING_IMAGES
        .lock()
        .entry(ctx as usize)
        .or_default()
        .push(op);
}

pub unsafe fn has_pending(ctx: *mut qjs::JSContext) -> bool {
    PENDING
        .lock()
        .get(&(ctx as usize))
        .is_some_and(|ops| !ops.is_empty())
        || PENDING_IMAGES
            .lock()
            .get(&(ctx as usize))
            .is_some_and(|ops| !ops.is_empty())
}

unsafe fn take_pending(ctx: *mut qjs::JSContext, op_id: u32) -> Option<PendingOp> {
    let mut pending = PENDING.lock();
    let ops = pending.get_mut(&(ctx as usize))?;
    let pos = ops.iter().position(|p| p.op_id == op_id)?;
    Some(ops.remove(pos))
}

fn find_pending_owner(op_id: u32) -> Option<*mut qjs::JSContext> {
    PENDING
        .lock()
        .iter()
        .find_map(|(ctx_key, ops)| ops.iter().any(|p| p.op_id == op_id).then_some(*ctx_key))
        .map(|ctx_key| ctx_key as *mut qjs::JSContext)
}

fn find_pending_image_owner(op_id: u32) -> Option<*mut qjs::JSContext> {
    PENDING_IMAGES
        .lock()
        .iter()
        .find_map(|(ctx_key, ops)| {
            ops.iter().any(|p| {
                matches!(
                    &p.stage,
                    PendingImageStage::SourceBytes { op_id: pending_id, .. } if *pending_id == op_id
                )
            })
            .then_some(*ctx_key)
        })
        .map(|ctx_key| ctx_key as *mut qjs::JSContext)
}

fn push_completed(ctx: *mut qjs::JSContext, op_id: u32) {
    COMPLETED
        .lock()
        .entry(ctx as usize)
        .or_default()
        .push(op_id);
}

fn take_completed_for_ctx(ctx: *mut qjs::JSContext) -> Option<u32> {
    let mut completed = COMPLETED.lock();
    let ops = completed.get_mut(&(ctx as usize))?;
    if ops.is_empty() {
        None
    } else {
        Some(ops.remove(0))
    }
}

unsafe fn reject_with_code(ctx: *mut qjs::JSContext, op: &PendingOp, code: i32) {
    let arg = js_int32(code);
    let _ = qjs::jsbind::call1(ctx, op.reject, qjs::JSValue::undefined(), arg);
}

unsafe fn resolve_with_value(ctx: *mut qjs::JSContext, op: &PendingOp, val: qjs::JSValue) {
    let _ = qjs::jsbind::call1(ctx, op.resolve, qjs::JSValue::undefined(), val);
    qjs::js_free_value(ctx, val);
}

unsafe fn resolve_undefined(ctx: *mut qjs::JSContext, op: &PendingOp) {
    let val = qjs::JSValue::undefined();
    let _ = qjs::jsbind::call1(ctx, op.resolve, qjs::JSValue::undefined(), val);
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
        unsafe { platform::gfx::upload_texture_png_async(tex_id, bytes.as_ptr(), bytes.len()) };
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
        unsafe { platform::gfx::upload_texture_jpeg_async(tex_id, bytes.as_ptr(), bytes.len()) };
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

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_bytes(bytes: &[u8]) -> Result<Vec<u8>, i32> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(-7);
            }
            let hi = hex_nibble(bytes[i + 1]).ok_or(-7)?;
            let lo = hex_nibble(bytes[i + 2]).ok_or(-7)?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(out)
}

fn data_url_svg_payload(bytes: &[u8]) -> Option<&[u8]> {
    let mut start = 0usize;
    while start < bytes.len() && matches!(bytes[start], b' ' | b'\t' | b'\r' | b'\n') {
        start += 1;
    }
    let trimmed = &bytes[start..];
    if trimmed.len() < 5 || !trimmed[..5].eq_ignore_ascii_case(b"data:") {
        return None;
    }
    let comma = trimmed.iter().position(|b| *b == b',')?;
    let meta = &trimmed[5..comma];
    let mut is_svg = false;
    for window in meta.windows(b"image/svg+xml".len()) {
        if window.eq_ignore_ascii_case(b"image/svg+xml") {
            is_svg = true;
            break;
        }
    }
    if !is_svg {
        return None;
    }
    Some(&trimmed[comma + 1..])
}

fn normalized_svg_upload_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    if svg_like_bytes(bytes) {
        return None;
    }

    if let Some(payload) = data_url_svg_payload(bytes) {
        if let Ok(decoded) = percent_decode_bytes(payload)
            && svg_like_bytes(&decoded)
        {
            return Some(decoded);
        }
        return None;
    }

    if bytes.iter().any(|b| *b == b'%')
        && let Ok(decoded) = percent_decode_bytes(bytes)
        && svg_like_bytes(&decoded)
    {
        return Some(decoded);
    }

    None
}

fn queue_svg_texture_upload(tex_id: u32, bytes: &[u8]) -> Result<(), i32> {
    let decoded = normalized_svg_upload_bytes(bytes);
    let upload_bytes = decoded.as_deref().unwrap_or(bytes);
    let rc = unsafe {
        platform::gfx::upload_texture_svg_async(
            tex_id,
            upload_bytes.as_ptr(),
            upload_bytes.len(),
        )
    };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

fn queue_image_texture_upload(
    op: &mut PendingImageOp,
    path: &[u8],
    bytes: &[u8],
) -> Result<(), i32> {
    let is_svg = path_has_svg_suffix(path) || svg_like_bytes(bytes) || data_url_svg_payload(bytes).is_some();
    let is_jpeg = !is_svg && (path_has_jpeg_suffix(path) || jpeg_like_bytes(bytes));
    if is_svg {
        queue_svg_texture_upload(op.tex_id, bytes).map(|_| {
            op.mime = String::from("image/svg+xml");
            op.width = 0;
            op.height = 0;
        })
    } else if is_jpeg {
        queue_jpeg_texture_upload(op.tex_id, bytes).map(|_| {
            op.mime = String::from("image/jpeg");
            op.width = 0;
            op.height = 0;
        })
    } else {
        queue_png_texture_upload(op.tex_id, bytes).map(|(width, height)| {
            op.mime = String::from("image/png");
            op.width = width;
            op.height = height;
        })
    }
}

fn texture_dimensions(tex_id: u32) -> Option<(u32, u32)> {
    let mut width = 0u32;
    let mut height = 0u32;
    if let Some((tex_width, tex_height)) = platform::gfx::texture_dimensions(tex_id) {
        width = tex_width;
        height = tex_height;
    }
    if width > 0 && height > 0 {
        Some((width, height))
    } else {
        None
    }
}

unsafe fn resolve_image_op(ctx: *mut qjs::JSContext, op: &PendingImageOp) {
    let obj = qjs::JS_NewObject(ctx);
    let _ = qjs::jsbind::set_prop(ctx, obj, b"texId\0", qjs::JS_NewFloat64(ctx, op.tex_id as f64));
    let _ = qjs::jsbind::set_prop(ctx, obj, b"width\0", qjs::JS_NewFloat64(ctx, op.width as f64));
    let _ = qjs::jsbind::set_prop(ctx, obj, b"height\0", qjs::JS_NewFloat64(ctx, op.height as f64));
    let _ = qjs::jsbind::set_str_prop(ctx, obj, b"mime\0", op.mime.as_str());
    let _ = qjs::jsbind::call1(ctx, op.resolve, qjs::JSValue::undefined(), obj);
    qjs::js_free_value(ctx, obj);
}

unsafe fn resolve_cached_image(
    ctx: *mut qjs::JSContext,
    resolve: qjs::JSValue,
    cached: &ReadyImageCacheEntry,
) {
    let obj = cached_image_object(ctx, cached);
    let _ = qjs::jsbind::call1(ctx, resolve, qjs::JSValue::undefined(), obj);
    qjs::js_free_value(ctx, obj);
}

unsafe fn cached_image_object(
    ctx: *mut qjs::JSContext,
    cached: &ReadyImageCacheEntry,
) -> qjs::JSValue {
    let obj = qjs::JS_NewObject(ctx);
    let _ = qjs::jsbind::set_prop(
        ctx,
        obj,
        b"texId\0",
        qjs::JS_NewFloat64(ctx, cached.tex_id as f64),
    );
    let _ = qjs::jsbind::set_prop(
        ctx,
        obj,
        b"width\0",
        qjs::JS_NewFloat64(ctx, cached.width as f64),
    );
    let _ = qjs::jsbind::set_prop(
        ctx,
        obj,
        b"height\0",
        qjs::JS_NewFloat64(ctx, cached.height as f64),
    );
    let _ = qjs::jsbind::set_str_prop(ctx, obj, b"mime\0", cached.mime.as_str());
    obj
}

pub unsafe fn cached_ready_image_texture_object(
    ctx: *mut qjs::JSContext,
    key: &[u8],
) -> Option<qjs::JSValue> {
    if key.is_empty() {
        return None;
    }
    let cached = {
        let cache = READY_IMAGE_CACHE.lock();
        cache
            .get(&(ctx as usize))
            .and_then(|entries| entries.iter().find(|entry| entry.key.as_slice() == key).cloned())
    }?;
    if image_diag_allowed() {
        image_diag_log(&format!(
            "img-async: cache-hit tex_id={} path_len={}\n",
            cached.tex_id,
            key.len()
        ));
    }
    Some(cached_image_object(ctx, &cached))
}

pub unsafe fn try_resolve_cached_ready_image_texture(
    ctx: *mut qjs::JSContext,
    resolve: qjs::JSValue,
    key: &[u8],
    unused_tex_id: u32,
) -> bool {
    if key.is_empty() {
        return false;
    }
    let cached = {
        let cache = READY_IMAGE_CACHE.lock();
        cache
            .get(&(ctx as usize))
            .and_then(|entries| entries.iter().find(|entry| entry.key.as_slice() == key).cloned())
    };
    if let Some(cached) = cached {
        if image_diag_allowed() {
            image_diag_log(&format!(
                "img-async: cache-hit tex_id={} path_len={}\n",
                cached.tex_id,
                key.len()
            ));
        }
        resolve_cached_image(ctx, resolve, &cached);
        crate::cmd_stream::release_managed_tex_id(unused_tex_id);
        return true;
    }
    false
}

fn remember_ready_image(ctx: *mut qjs::JSContext, op: &PendingImageOp) {
    let Some(key) = &op.cache_key else {
        return;
    };
    if key.is_empty() || op.tex_id == 0 || op.width == 0 || op.height == 0 {
        return;
    }
    let mut cache = READY_IMAGE_CACHE.lock();
    let entries = cache.entry(ctx as usize).or_default();
    if let Some(entry) = entries.iter_mut().find(|entry| entry.key == *key) {
        entry.tex_id = op.tex_id;
        entry.width = op.width;
        entry.height = op.height;
        entry.mime = op.mime.clone();
        return;
    }
    entries.push(ReadyImageCacheEntry {
        key: key.clone(),
        tex_id: op.tex_id,
        width: op.width,
        height: op.height,
        mime: op.mime.clone(),
    });
}

unsafe fn reject_image_op(ctx: *mut qjs::JSContext, op: &PendingImageOp, code: i32) {
    let arg = js_int32(code);
    let _ = qjs::jsbind::call1(ctx, op.reject, qjs::JSValue::undefined(), arg);
}

unsafe fn pump_image_requests(ctx: *mut qjs::JSContext) -> bool {
    let mut progress = false;
    let mut ops: Vec<PendingImageOp> = Vec::new();
    {
        let mut pending = PENDING_IMAGES.lock();
        let key = ctx as usize;
        if let Some(entries) = pending.get_mut(&key) {
            core::mem::swap(entries, &mut ops);
        }
    }

    for mut op in ops {
        let mut keep = false;
        match &mut op.stage {
            PendingImageStage::CachedReady { cached } => {
                progress = true;
                resolve_cached_image(ctx, op.resolve, cached);
            }
            PendingImageStage::InlineBytes {
                path,
                bytes,
                source,
            } => {
                progress = true;
                if image_diag_allowed() {
                    image_diag_log(&format!(
                        "img-async: bytes-inline tex_id={} source={:?} len={}\n",
                        op.tex_id,
                        source,
                        bytes.len()
                    ));
                }
                let path = core::mem::take(path);
                let bytes = core::mem::take(bytes);
                match queue_image_texture_upload(&mut op, path.as_slice(), bytes.as_slice()) {
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
                        ImageRequestSource::InlineData => {}
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
                            let path = core::mem::take(path);
                            match queue_image_texture_upload(
                                &mut op,
                                path.as_slice(),
                                bytes.as_slice(),
                            ) {
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
                let status = platform::gfx::texture_status(op.tex_id);
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
                    remember_ready_image(ctx, &op);
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
            PENDING_IMAGES
                .lock()
                .entry(ctx as usize)
                .or_default()
                .push(op);
        } else {
            qjs::js_free_value(ctx, op.resolve);
            qjs::js_free_value(ctx, op.reject);
        }
    }

    progress
}

/// Pump only image texture requests for callers that need bounded scene progress.
///
/// This intentionally avoids resolving unrelated JS jobs/timers/workers; image
/// fidelity must not be able to block a page scene handoff.
pub unsafe fn pump_images(ctx: *mut qjs::JSContext) -> bool {
    if ctx.is_null() {
        return false;
    }
    pump_image_requests(ctx)
}

unsafe fn pump_net_fetch_text(ctx: *mut qjs::JSContext) -> bool {
    // Net ops don't show up in async_fs::poll_completed(), so we must poll their result here.
    let mut progress = false;

    // Collect op_ids to process to avoid holding the mutex while doing work.
    let op_ids: Vec<u32> = {
        let pending = PENDING.lock();
        pending
            .get(&(ctx as usize))
            .map(|ops| {
                ops.iter()
                    .filter(|p| {
                        p.kind == OpKind::NetFetchTextFile || p.kind == OpKind::NetPostJsonTextBytes
                    })
                    .map(|p| p.op_id)
                    .collect()
            })
            .unwrap_or_default()
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

        match op.kind {
            OpKind::NetFetchTextFile => {
                // Consume the net op result (also discards the kernel op id), then read the
                // cache file back directly. Routing this through async_fs_service can stall on
                // nested sync-over-async readback even though the network request already finished.
                let _ = async_fs::read_result(op_id, core::ptr::null_mut(), 0);
                match read_file_via_cabi(op.aux.as_slice()) {
                    Ok(buf) => {
                        crate::trueos_shims::log_info(
                            format!(
                                "qjs fetch: readback ok bytes={} request_id={}\n",
                                buf.len(),
                                op_id
                            )
                            .as_str(),
                        );
                        let s = qjs::JS_NewStringLen(
                            ctx,
                            buf.as_ptr() as *const core::ffi::c_char,
                            buf.len(),
                        );
                        resolve_with_value(ctx, &op, s);
                    }
                    Err(code) => {
                        crate::trueos_shims::log_error(
                            format!(
                                "qjs fetch: readback failed rc={} request_id={}\n",
                                code, op_id
                            )
                            .as_str(),
                        );
                        reject_with_code(ctx, &op, code)
                    }
                }
            }
            OpKind::NetPostJsonTextBytes => {
                let n = rc_or_done as usize;
                match read_completion_bytes(op_id, n) {
                    Ok(buf) => {
                        crate::trueos_shims::log_info(
                            format!(
                                "qjs fetch: body ready bytes={} request_id={}\n",
                                buf.len(),
                                op_id
                            )
                            .as_str(),
                        );
                        let s = qjs::JS_NewStringLen(
                            ctx,
                            buf.as_ptr() as *const core::ffi::c_char,
                            buf.len(),
                        );
                        resolve_with_value(ctx, &op, s);
                    }
                    Err(code) => {
                        crate::trueos_shims::log_error(
                            format!(
                                "qjs fetch: body read failed rc={} request_id={}\n",
                                code, op_id
                            )
                            .as_str(),
                        );
                        reject_with_code(ctx, &op, code)
                    }
                }
            }
            _ => {}
        }
        if !op.aux.is_empty() {
            let _ = trueos_cabi_fs_remove(op.aux.as_ptr(), op.aux.len());
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
            .get(&(ctx as usize))
            .map(|ops| {
                ops.iter()
                    .filter(|p| p.kind == OpKind::NetFetchBytes)
                    .map(|p| p.op_id)
                    .collect()
            })
            .unwrap_or_default()
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

unsafe fn pump_net_fetch_module(ctx: *mut qjs::JSContext) -> bool {
    let mut progress = false;

    let op_ids: Vec<u32> = {
        let pending = PENDING.lock();
        pending
            .get(&(ctx as usize))
            .map(|ops| {
                ops.iter()
                    .filter(|p| p.kind == OpKind::NetFetchModule)
                    .map(|p| p.op_id)
                    .collect()
            })
            .unwrap_or_default()
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
        let spec =
            qjs::JS_NewStringLen(ctx, op.aux.as_ptr() as *const core::ffi::c_char, op.aux.len());
        resolve_with_value(ctx, &op, spec);
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
    progress |= pump_net_fetch_module(ctx);

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
            OpKind::NetFetchTextFile | OpKind::NetPostJsonTextBytes => {
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
            OpKind::NetFetchModule => {
                // Net fetches are handled by `pump_net_fetch_module`.
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
    let pending = PENDING.lock().remove(&(ctx as usize)).unwrap_or_default();
    for op in pending {
        reject_with_code(ctx, &op, -2);
        qjs::js_free_value(ctx, op.resolve);
        qjs::js_free_value(ctx, op.reject);
        let _ = async_fs::discard(op.op_id);
    }

    let pending_images = PENDING_IMAGES
        .lock()
        .remove(&(ctx as usize))
        .unwrap_or_default();
    for op in pending_images {
        let release_tex = !matches!(op.stage, PendingImageStage::CachedReady { .. });
        if release_tex {
            reject_image_op(ctx, &op, -2);
        }
        qjs::js_free_value(ctx, op.resolve);
        qjs::js_free_value(ctx, op.reject);
        match op.stage {
            PendingImageStage::CachedReady { .. } => {}
            PendingImageStage::InlineBytes { .. } => {}
            PendingImageStage::SourceBytes { op_id, path, .. } => {
                let _ = async_fs::discard(op_id);
                if !path.is_empty() {
                    let _ = unsafe { trueos_cabi_fs_remove(path.as_ptr(), path.len()) };
                }
            }
            PendingImageStage::Upload => {}
        }
        if release_tex {
            crate::cmd_stream::release_managed_tex_id(op.tex_id);
        }
    }

    let discard_ids = COMPLETED.lock().remove(&(ctx as usize)).unwrap_or_default();
    for op_id in discard_ids {
        let _ = async_fs::discard(op_id);
    }
}

pub async fn wait_for_completion(timeout_ms: u64) -> bool {
    async_fs::wait_for_completion(timeout_ms).await
}
