#![cfg(feature = "trueos")]

use core::ffi::c_char;

use crate as qjs;

// Fresh wiring: no browser_webgpu native module yet.
pub unsafe fn try_create_native_module(
    _ctx: *mut qjs::JSContext,
    _module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    core::ptr::null_mut()
}
