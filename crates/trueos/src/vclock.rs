extern crate alloc;

use alloc::{string::String, vec};

use crate::vcabi;

#[inline]
pub fn ntp_current_unix_seconds() -> u64 {
    unsafe { vcabi::trueos_cabi_ntp_current_unix_seconds() }
}

#[inline]
pub fn kernel_date_day_month_year() -> Option<String> {
    let len = unsafe {
        vcabi::trueos_cabi_ntp_kernel_date_day_month_year(core::ptr::null_mut(), 0)
    };
    if len == 0 {
        return None;
    }
    let mut bytes = vec![0u8; len];
    let got = unsafe {
        vcabi::trueos_cabi_ntp_kernel_date_day_month_year(bytes.as_mut_ptr(), bytes.len())
    };
    if got == 0 {
        return None;
    }
    bytes.truncate(got);
    String::from_utf8(bytes).ok()
}
