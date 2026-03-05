#![cfg(feature = "trueos")]

use crate as qjs;

// Fresh minimal browser bridge: mouse API is intentionally omitted.
pub unsafe fn install_mouse_api(_ctx: *mut qjs::JSContext, _target: qjs::JSValueConst) {}
