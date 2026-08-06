//! QuickJS-facing TRUEOS ABI overlay.
//!
//! This intentionally reuses the BP service ABI for OS services while keeping
//! QuickJS-specific JS runtime declarations inside `trueos-qjs`.

pub use crate::bp_abi::*;
pub use crate::legacy_fs_abi::*;

unsafe extern "C" {
    pub fn trueos_cabi_boot_timestamp_secs() -> u64;
}
