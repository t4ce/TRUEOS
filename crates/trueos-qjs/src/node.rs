#![cfg(feature = "trueos")]

use core::ffi::{c_char, c_void};

use crate as qjs;

unsafe extern "C" fn trueos_node_module_normalize(
    ctx: *mut qjs::JSContext,
    module_base_name: *const c_char,
    module_name: *const c_char,
    _opaque: *mut c_void,
) -> *mut c_char {
    // Delegate to the shared TRUEOS normalizer in Node mode.
    qjs::trueos_module_loader::normalize_with_mode(
        ctx,
        module_base_name,
        module_name,
        qjs::trueos_module_loader::NormalizeMode::Node,
    )
}

/// Install the TRUEOS module loader with Node-ish specifier resolution.
///
/// This composes the existing TRUEOS loader (`trueos_modules::trueos_module_loader`) but
/// upgrades normalization rules:
/// - Some Node builtins are provided natively (e.g. `process`, `path`).
/// - Other common Node builtins (e.g. `events`, `util`, ...) are routed to pinned polyfill
///   packages on esm.sh (since esm.sh does not serve `node:*` specifiers directly).
/// - Unknown `node:*` specifiers are routed through esm.sh by stripping the `node:` prefix.
pub unsafe fn install(rt: *mut qjs::JSRuntime) {
    if rt.is_null() {
        return;
    }

    qjs::JS_SetModuleLoaderFunc(
        rt,
        Some(trueos_node_module_normalize),
        Some(qjs::trueos_module_loader::trueos_module_loader),
        core::ptr::null_mut(),
    );
}

/// Convenience wrapper: Node mode currently reuses the same globals as the base loader.
pub unsafe fn install_globals(ctx: *mut qjs::JSContext) {
    let _ = ctx;
}
