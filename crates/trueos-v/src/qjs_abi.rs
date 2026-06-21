//! QuickJS-facing TRUEOS ABI overlay.
//!
//! This intentionally reuses the BP service ABI for OS services while keeping
//! QuickJS-specific JS runtime declarations inside `trueos-qjs`.

pub use crate::bp_abi::*;

unsafe extern "C" {
    pub fn trueos_cabi_gfx_capture_screenshot_data_url(out_ptr: *mut u8, out_cap: usize) -> isize;
    pub fn trueos_cabi_gfx_texture_dimensions(
        tex_id: u32,
        out_width: *mut u32,
        out_height: *mut u32,
    ) -> i32;
    pub fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32;
    pub fn trueos_cabi_boot_timestamp_secs() -> u64;
    pub fn trueos_cabi_browser_asset_refs_begin(browser_instance_id: u32) -> i32;
    pub fn trueos_cabi_browser_asset_ref_push(
        browser_instance_id: u32,
        tag_ptr: *const u8,
        tag_len: usize,
        url_ptr: *const u8,
        url_len: usize,
        kind_ptr: *const u8,
        kind_len: usize,
    ) -> i32;
}
