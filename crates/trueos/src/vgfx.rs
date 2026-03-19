extern crate alloc;

use alloc::{string::String, vec};

use crate::vcabi;

#[inline]
pub fn capture_screenshot_data_url() -> Option<String> {
    let len =
        unsafe { vcabi::trueos_cabi_gfx_capture_screenshot_data_url(core::ptr::null_mut(), 0) };
    if len <= 0 {
        return None;
    }

    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_gfx_capture_screenshot_data_url(bytes.as_mut_ptr(), bytes.len())
    };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).ok()
}
