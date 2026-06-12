use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use spin::Mutex;

const UI3_IMG_STATUS_UNKNOWN: i32 = 0;
const UI3_IMG_STATUS_PENDING: i32 = 1;
const UI3_IMG_STATUS_READY: i32 = 2;

#[derive(Clone)]
pub(crate) struct Ui3Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

static UI3_IMAGES: Mutex<BTreeMap<u32, Ui3Image>> = Mutex::new(BTreeMap::new());
static UI3_IMAGE_STATUS: Mutex<BTreeMap<u32, i32>> = Mutex::new(BTreeMap::new());

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
pub extern "C" fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32 {
    image_status(tex_id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_texture_dimensions(
    tex_id: u32,
    out_width: *mut u32,
    out_height: *mut u32,
) -> i32 {
    let Some((width, height)) = image_dimensions(tex_id) else {
        return UI3_IMG_STATUS_UNKNOWN;
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
    UI3_IMG_STATUS_READY
}
