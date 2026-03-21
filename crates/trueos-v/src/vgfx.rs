extern crate alloc;

use alloc::{string::String, vec};

use crate::vcabi;

#[inline]
pub fn upload_svg_to_texture(tex_id: u32, svg: &[u8]) -> i32 {
    if svg.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_svg(tex_id, svg.as_ptr(), svg.len()) }
}

#[inline]
pub fn upload_svg_to_texture_async(tex_id: u32, svg: &[u8]) -> i32 {
    if svg.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_svg_async(tex_id, svg.as_ptr(), svg.len()) }
}

#[inline]
pub fn probe_upload_svg_to_texture_async(tex_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_svg_async(tex_id, core::ptr::null(), 0) }
}

#[inline]
pub fn texture_status(tex_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_gfx_texture_status(tex_id) }
}

#[inline]
pub fn capture_screenshot_data_url() -> Option<String> {
    let len = unsafe { vcabi::trueos_cabi_gfx_capture_screenshot_data_url(core::ptr::null_mut(), 0) };
    if len <= 0 {
        return None;
    }

    let mut bytes = vec![0u8; len as usize];
    let got =
        unsafe { vcabi::trueos_cabi_gfx_capture_screenshot_data_url(bytes.as_mut_ptr(), bytes.len()) };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).ok()
}
