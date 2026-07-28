//! QuickJS-facing TRUEOS ABI overlay.
//!
//! This intentionally reuses the BP service ABI for OS services while keeping
//! QuickJS-specific JS runtime declarations inside `trueos-qjs`.

pub use crate::bp_abi::*;
pub use crate::legacy_fs_abi::*;

unsafe extern "C" {
    pub fn trueos_cabi_gfx_texture_dimensions(
        tex_id: u32,
        out_width: *mut u32,
        out_height: *mut u32,
    ) -> i32;
    pub fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32;
    pub fn trueos_cabi_boot_timestamp_secs() -> u64;
}
