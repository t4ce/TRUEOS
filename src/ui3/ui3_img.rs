use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

const UI3_IMG_STATUS_UNKNOWN: i32 = 0;
const UI3_IMG_STATUS_PENDING: i32 = 1;
const UI3_IMG_STATUS_READY: i32 = 2;
const UI3_IMG_ERR_NULL: i32 = -2;
const UI3_IMG_ERR_EMPTY: i32 = -3;

#[derive(Clone)]
pub(crate) struct Ui3Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

static UI3_IMAGES: Mutex<BTreeMap<u32, Ui3Image>> = Mutex::new(BTreeMap::new());
static UI3_IMAGE_STATUS: Mutex<BTreeMap<u32, i32>> = Mutex::new(BTreeMap::new());
static UI3_IMAGE_UPLOADS: Mutex<BTreeMap<(u8, u32), Ui3ImageUpload>> = Mutex::new(BTreeMap::new());
static UI3_UPLOAD_FINISH_COUNT: AtomicU64 = AtomicU64::new(0);
static UI3_UPLOAD_BYTES_TOTAL: AtomicU64 = AtomicU64::new(0);

struct Ui3ImageUpload {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    written: usize,
}

fn valid_tex_id(tex_id: u32) -> bool {
    tex_id != 0
}

fn expected_rgba_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

fn set_status(tex_id: u32, status: i32) {
    if !valid_tex_id(tex_id) {
        return;
    }
    UI3_IMAGE_STATUS.lock().insert(tex_id, status);
}

fn set_error(tex_id: u32, status: i32) -> i32 {
    set_status(tex_id, status);
    status
}

unsafe fn encoded_bytes<'a>(data_ptr: *const u8, data_len: usize) -> Result<&'a [u8], i32> {
    if data_ptr.is_null() {
        return Err(UI3_IMG_ERR_NULL);
    }
    if data_len == 0 {
        return Err(UI3_IMG_ERR_EMPTY);
    }
    Ok(unsafe { core::slice::from_raw_parts(data_ptr, data_len) })
}

#[inline]
fn running_in_hull_guest() -> bool {
    crate::hv::current_hull_guest_context_vm_id().is_some()
}

fn vmcall_texture_rc(op: u32, arg0: u64, arg1: u64, payload: &[u8]) -> i32 {
    let (status, data) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, payload, &mut []);
    if status == trueos_vm::vmcall::STATUS_OK {
        data as i64 as i32
    } else {
        -1
    }
}

fn upload_rgba_image_to_host(
    tex_id: u32,
    width: u32,
    height: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    if data_ptr.is_null() {
        return -2;
    }
    let Some(expected) = expected_rgba_len(width, height) else {
        return -7;
    };
    if data_len < expected {
        return -3;
    }
    let total_len = (expected as u64).to_le_bytes();
    let rc = vmcall_texture_rc(
        trueos_vm::vmcall::OP_BP_UI3_TEXTURE_UPLOAD_BEGIN,
        tex_id as u64,
        ((width as u64) << 32) | (height as u64),
        &total_len,
    );
    if rc != 0 {
        return rc;
    }

    let src = unsafe { core::slice::from_raw_parts(data_ptr, expected) };
    let mut offset = 0usize;
    while offset < expected {
        let end = core::cmp::min(offset.saturating_add(trueos_vm::vmcall::PAYLOAD_CAP), expected);
        let rc = vmcall_texture_rc(
            trueos_vm::vmcall::OP_BP_UI3_TEXTURE_UPLOAD_CHUNK,
            tex_id as u64,
            offset as u64,
            &src[offset..end],
        );
        if rc != 0 {
            return rc;
        }
        offset = end;
    }
    vmcall_texture_rc(trueos_vm::vmcall::OP_BP_UI3_TEXTURE_UPLOAD_FINISH, tex_id as u64, 0, &[])
}

#[cfg(feature = "trueos_rdp")]
fn preserve_encoded_texture(
    tex_id: u32,
    kind: crate::r::resource_monitor::EncodedKind,
    flags: u32,
    bytes: &[u8],
) {
    let _ = crate::r::resource_monitor::preserve_encoded_texture(tex_id, kind, flags, bytes);
}

#[cfg(not(feature = "trueos_rdp"))]
fn preserve_encoded_texture(_tex_id: u32, _kind: (), _flags: u32, _bytes: &[u8]) {}

fn upload_decoded_rgba(tex_id: u32, width: u32, height: u32, rgba: Vec<u8>) -> i32 {
    set_status(tex_id, UI3_IMG_STATUS_PENDING);
    let rc = store_rgba_image(tex_id, width, height, rgba);
    if rc != 0 { set_error(tex_id, rc) } else { rc }
}

fn upload_png_bytes_to_texture(tex_id: u32, bytes: &[u8], flags: u32) -> i32 {
    #[cfg(feature = "trueos_rdp")]
    preserve_encoded_texture(tex_id, crate::r::resource_monitor::EncodedKind::Png, flags, bytes);
    #[cfg(not(feature = "trueos_rdp"))]
    preserve_encoded_texture(tex_id, (), flags, bytes);

    match crate::ui3::img::png_codec::decode_png_rgba(bytes) {
        Ok(decoded) => upload_decoded_rgba(tex_id, decoded.width, decoded.height, decoded.rgba),
        Err(err) => set_error(tex_id, err.code()),
    }
}

fn upload_jpeg_bytes_to_texture(tex_id: u32, bytes: &[u8], flags: u32) -> i32 {
    #[cfg(feature = "trueos_rdp")]
    preserve_encoded_texture(tex_id, crate::r::resource_monitor::EncodedKind::Jpeg, flags, bytes);
    #[cfg(not(feature = "trueos_rdp"))]
    preserve_encoded_texture(tex_id, (), flags, bytes);

    match crate::ui3::img::jpeg_codec::decode_jpeg_rgba(bytes) {
        Ok(decoded) => upload_decoded_rgba(tex_id, decoded.width, decoded.height, decoded.rgba),
        Err(err) => set_error(tex_id, err.code()),
    }
}

fn upload_svg_bytes_to_texture_rgba(tex_id: u32, bytes: &[u8], flags: u32) -> i32 {
    #[cfg(feature = "trueos_rdp")]
    preserve_encoded_texture(tex_id, crate::r::resource_monitor::EncodedKind::Svg, flags, bytes);
    #[cfg(not(feature = "trueos_rdp"))]
    preserve_encoded_texture(tex_id, (), flags, bytes);

    match crate::ui3::img::svg::render_svg_bytes_rgba(bytes) {
        Ok((info, rgba)) => upload_decoded_rgba(tex_id, info.width, info.height, rgba),
        Err(err) => set_error(tex_id, err),
    }
}

pub(crate) fn store_rgba_image(tex_id: u32, width: u32, height: u32, rgba: Vec<u8>) -> i32 {
    if !valid_tex_id(tex_id) {
        return -1;
    }
    if width == 0 || height == 0 {
        return -1;
    }
    let Some(expected) = expected_rgba_len(width, height) else {
        return -7;
    };
    if rgba.len() < expected {
        return -3;
    }

    let mut image = rgba;
    image.truncate(expected);
    UI3_IMAGES.lock().insert(
        tex_id,
        Ui3Image {
            width,
            height,
            rgba: image,
        },
    );
    set_status(tex_id, UI3_IMG_STATUS_READY);
    0
}

pub(crate) fn image_clone(tex_id: u32) -> Option<Ui3Image> {
    UI3_IMAGES.lock().get(&tex_id).cloned()
}

pub(crate) fn begin_rgba_upload(
    vm_id: u8,
    tex_id: u32,
    width: u32,
    height: u32,
    total_len: usize,
) -> i32 {
    if !valid_tex_id(tex_id) || width == 0 || height == 0 {
        return -1;
    }
    let Some(expected) = expected_rgba_len(width, height) else {
        return -7;
    };
    if total_len < expected {
        return -3;
    }
    set_status(tex_id, UI3_IMG_STATUS_PENDING);
    let mut rgba = Vec::new();
    rgba.resize(expected, 0);
    UI3_IMAGE_UPLOADS.lock().insert(
        (vm_id, tex_id),
        Ui3ImageUpload {
            width,
            height,
            rgba,
            written: 0,
        },
    );
    0
}

pub(crate) fn write_rgba_upload_chunk(vm_id: u8, tex_id: u32, offset: usize, bytes: &[u8]) -> i32 {
    let mut uploads = UI3_IMAGE_UPLOADS.lock();
    let Some(upload) = uploads.get_mut(&(vm_id, tex_id)) else {
        return -1;
    };
    let Some(end) = offset.checked_add(bytes.len()) else {
        return -7;
    };
    if end > upload.rgba.len() {
        return -3;
    }
    upload.rgba[offset..end].copy_from_slice(bytes);
    upload.written = upload.written.max(end);
    0
}

pub(crate) fn finish_rgba_upload(vm_id: u8, tex_id: u32) -> i32 {
    let upload = UI3_IMAGE_UPLOADS.lock().remove(&(vm_id, tex_id));
    let Some(upload) = upload else {
        return -1;
    };
    if upload.written < upload.rgba.len() {
        return -3;
    }
    let byte_len = upload.rgba.len() as u64;
    let width = upload.width;
    let height = upload.height;
    let rc = store_rgba_image(tex_id, width, height, upload.rgba);
    if rc == 0 {
        let count = UI3_UPLOAD_FINISH_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        let total = UI3_UPLOAD_BYTES_TOTAL.fetch_add(byte_len, Ordering::Relaxed) + byte_len;
        if count <= 16 || count % 64 == 0 {
            crate::log!(
                "ui3/img: upload#{} vm={} tex={} size={}x{} bytes={} total_bytes={}\n",
                count,
                vm_id,
                tex_id,
                width,
                height,
                byte_len,
                total
            );
        }
    }
    rc
}

pub(crate) fn image_dimensions(tex_id: u32) -> Option<(u32, u32)> {
    UI3_IMAGES
        .lock()
        .get(&tex_id)
        .map(|image| (image.width, image.height))
}

pub(crate) fn image_status(tex_id: u32) -> i32 {
    if !valid_tex_id(tex_id) {
        return UI3_IMG_STATUS_UNKNOWN;
    }
    let status = UI3_IMAGE_STATUS
        .lock()
        .get(&tex_id)
        .copied()
        .unwrap_or(UI3_IMG_STATUS_UNKNOWN);
    if status != UI3_IMG_STATUS_UNKNOWN {
        return status;
    }
    if image_dimensions(tex_id).is_some() {
        UI3_IMG_STATUS_READY
    } else {
        UI3_IMG_STATUS_UNKNOWN
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_rgba_image(
    tex_id: u32,
    width: u32,
    height: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    if running_in_hull_guest() {
        return upload_rgba_image_to_host(tex_id, width, height, data_ptr, data_len);
    }
    if data_ptr.is_null() {
        return -2;
    }
    let Some(expected) = expected_rgba_len(width, height) else {
        return -7;
    };
    if data_len < expected {
        return -3;
    }
    set_status(tex_id, UI3_IMG_STATUS_PENDING);
    let rgba = unsafe { core::slice::from_raw_parts(data_ptr, expected) }.to_vec();
    store_rgba_image(tex_id, width, height, rgba)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_rgba(
    tex_id: u32,
    width: u32,
    height: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    unsafe { trueos_cabi_gfx_upload_texture_rgba_image(tex_id, width, height, data_ptr, data_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_rgba_image_async(
    tex_id: u32,
    width: u32,
    height: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    unsafe { trueos_cabi_gfx_upload_texture_rgba_image(tex_id, width, height, data_ptr, data_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_png(
    tex_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let bytes = match unsafe { encoded_bytes(data_ptr, data_len) } {
        Ok(bytes) => bytes,
        Err(code) => return set_error(tex_id, code),
    };
    upload_png_bytes_to_texture(tex_id, bytes, 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_png_async(
    tex_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let bytes = match unsafe { encoded_bytes(data_ptr, data_len) } {
        Ok(bytes) => bytes,
        Err(code) => return set_error(tex_id, code),
    };
    upload_png_bytes_to_texture(tex_id, bytes, 1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_jpeg(
    tex_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let bytes = match unsafe { encoded_bytes(data_ptr, data_len) } {
        Ok(bytes) => bytes,
        Err(code) => return set_error(tex_id, code),
    };
    upload_jpeg_bytes_to_texture(tex_id, bytes, 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_jpeg_async(
    tex_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let bytes = match unsafe { encoded_bytes(data_ptr, data_len) } {
        Ok(bytes) => bytes,
        Err(code) => return set_error(tex_id, code),
    };
    upload_jpeg_bytes_to_texture(tex_id, bytes, 1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_svg(
    tex_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let bytes = match unsafe { encoded_bytes(data_ptr, data_len) } {
        Ok(bytes) => bytes,
        Err(code) => return set_error(tex_id, code),
    };
    upload_svg_bytes_to_texture_rgba(tex_id, bytes, 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_svg_async(
    tex_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let bytes = match unsafe { encoded_bytes(data_ptr, data_len) } {
        Ok(bytes) => bytes,
        Err(code) => return set_error(tex_id, code),
    };
    upload_svg_bytes_to_texture_rgba(tex_id, bytes, 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32 {
    if running_in_hull_guest() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI3_TEXTURE_STATUS,
            tex_id as u64,
            0,
            &[],
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            UI3_IMG_STATUS_UNKNOWN
        };
    }
    image_status(tex_id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_texture_dimensions(
    tex_id: u32,
    out_width: *mut u32,
    out_height: *mut u32,
) -> i32 {
    if running_in_hull_guest() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI3_TEXTURE_DIMENSIONS,
            tex_id as u64,
            0,
            &[],
            &mut [],
        );
        if status != trueos_vm::vmcall::STATUS_OK || data == 0 {
            return -1;
        }
        let width = (data >> 32) as u32;
        let height = data as u32;
        if !out_width.is_null() {
            unsafe {
                *out_width = width;
            }
        }
        if !out_height.is_null() {
            unsafe {
                *out_height = height;
            }
        }
        return 0;
    }
    let Some((width, height)) = image_dimensions(tex_id) else {
        return -1;
    };
    if !out_width.is_null() {
        unsafe {
            *out_width = width;
        }
    }
    if !out_height.is_null() {
        unsafe {
            *out_height = height;
        }
    }
    0
}
