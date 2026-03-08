#![cfg(feature = "trueos")]

use core::ffi::c_char;

use crate as qjs;

const LYON_CATALOG_LINES: &[&str] = &[
    "Type Aliases",
    "  Box2D (euclid::default::Box2D)",
    "  Point (euclid::default::Point2D)",
    "  Rotation (euclid::default::Rotation2D)",
    "  Scale (euclid::default::Scale)",
    "  Size (euclid::default::Size2D)",
    "  Transform (euclid::default::Transform2D)",
    "  Translation (euclid::default::Translation2D)",
    "  Vector (euclid::default::Vector2D)",
    "Structs",
    "  Angle",
    "  Arc",
    "  ArcFlags",
    "  CubicBezierSegment",
    "  Line",
    "  LineEquation",
    "  LineSegment",
    "  QuadraticBezierSegment",
    "  SvgArc",
    "  Triangle",
    "Re-exports",
    "  pub use arrayvec",
    "  pub use euclid",
    "Modules",
    "  arc",
    "  cubic_bezier",
    "  quadratic_bezier",
    "  traits",
    "  utils",
    "Tessellation Pattern",
    "  use lyon::math::{Box2D, Point, point};",
    "  use lyon::path::{Winding, builder::BorderRadii};",
    "  use lyon::tessellation::{FillTessellator, FillOptions, VertexBuffers};",
    "  let mut geometry: VertexBuffers<Point, u16> = VertexBuffers::new();",
    "  let options = FillOptions::tolerance(0.1);",
    "  builder.add_rounded_rectangle(&Box2D{...}, &BorderRadii{...}, Winding::Positive);",
    "  builder.build(); // geometry.vertices / geometry.indices ready",
    "",
];

#[inline]
unsafe fn js_str(ctx: *mut qjs::JSContext, s: &str) -> qjs::JSValue {
    qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len())
}

unsafe extern "C" fn qjs_lyon_catalog_lines(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let arr = qjs::JS_NewArray(ctx);
    if arr.is_exception() {
        return arr;
    }

    let mut i = 0u32;
    while (i as usize) < LYON_CATALOG_LINES.len() {
        let v = unsafe { js_str(ctx, LYON_CATALOG_LINES[i as usize]) };
        let _ = qjs::JS_SetPropertyUint32(ctx, arr, i, v);
        i += 1;
    }

    arr
}

pub unsafe fn install_lyon_api(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    static NAME: &[u8] = b"__trueosLyonCatalogLines\0";
    static FN_NAME: &[u8] = b"__trueosLyonCatalogLines\0";

    let global = qjs::JS_GetGlobalObject(ctx);
    let func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_lyon_catalog_lines),
        FN_NAME.as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, NAME.as_ptr() as *const c_char, func);
    qjs::js_free_value(ctx, global);
}
