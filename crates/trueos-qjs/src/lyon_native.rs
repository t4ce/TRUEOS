#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

use crate as qjs;

#[cfg(feature = "lyon-native")]
use libm::sqrtf;
#[cfg(feature = "lyon-native")]
use lyon_geom::{point, CubicBezierSegment, LineSegment, Point, QuadraticBezierSegment};
#[cfg(feature = "lyon-native")]
use lyon_tessellation::geometry_builder::simple_builder;
#[cfg(feature = "lyon-native")]
use lyon_tessellation::math::point as tess_point;
#[cfg(feature = "lyon-native")]
use lyon_tessellation::path::Path;
#[cfg(feature = "lyon-native")]
use lyon_tessellation::path::builder::BorderRadii;
#[cfg(feature = "lyon-native")]
use lyon_tessellation::{StrokeOptions, StrokeTessellator, VertexBuffers};

#[inline]
fn js_bool(v: bool) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion {
            int32: if v { 1 } else { 0 },
        },
        tag: qjs::JS_TAG_BOOL,
    }
}

#[inline]
fn js_num(ctx: *mut qjs::JSContext, v: f64) -> qjs::JSValue {
    unsafe { qjs::JS_NewFloat64(ctx, v) }
}

#[inline]
unsafe fn js_f32_array(ctx: *mut qjs::JSContext, vals: &[f32]) -> qjs::JSValue {
    let arr = unsafe { qjs::JS_NewArray(ctx) };
    if arr.is_exception() {
        return arr;
    }
    let mut i = 0u32;
    while (i as usize) < vals.len() {
        let v = js_num(ctx, vals[i as usize] as f64);
        let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, i, v) };
        i += 1;
    }
    arr
}

#[inline]
unsafe fn js_u16_array(ctx: *mut qjs::JSContext, vals: &[u16]) -> qjs::JSValue {
    let arr = unsafe { qjs::JS_NewArray(ctx) };
    if arr.is_exception() {
        return arr;
    }
    let mut i = 0u32;
    while (i as usize) < vals.len() {
        let v = js_num(ctx, vals[i as usize] as f64);
        let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, i, v) };
        i += 1;
    }
    arr
}

#[inline]
fn js_str(ctx: *mut qjs::JSContext, s: &str) -> qjs::JSValue {
    unsafe { qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len()) }
}

#[inline]
unsafe fn set_prop_str(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &str, val: qjs::JSValue) {
    let mut keyz = String::from(key);
    keyz.push('\0');
    let _ = unsafe { qjs::JS_SetPropertyStr(ctx, obj, keyz.as_ptr() as *const c_char, val) };
}

fn triangle_area(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f64 {
    let area2 = a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1);
    (0.5f32 * area2.abs()) as f64
}

#[cfg(feature = "lyon-native")]
fn triangle_signed_area(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f64 {
    let area2 = a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1);
    (0.5f32 * area2) as f64
}

#[cfg(feature = "lyon-native")]
fn point_distance(a: Point<f32>, b: Point<f32>) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    sqrtf(dx * dx + dy * dy) as f64
}

#[cfg(feature = "lyon-native")]
fn approx_quad_length(quad: &QuadraticBezierSegment<f32>, steps: u32) -> f64 {
    let n = if steps < 2 { 2 } else { steps };
    let mut acc = 0.0f64;
    let mut prev = quad.sample(0.0);
    let mut i = 1u32;
    while i <= n {
        let t = (i as f32) / (n as f32);
        let p = quad.sample(t);
        acc += point_distance(prev, p);
        prev = p;
        i += 1;
    }
    acc
}

#[cfg(feature = "lyon-native")]
fn approx_cubic_length(cubic: &CubicBezierSegment<f32>, steps: u32) -> f64 {
    let n = if steps < 2 { 2 } else { steps };
    let mut acc = 0.0f64;
    let mut prev = cubic.sample(0.0);
    let mut i = 1u32;
    while i <= n {
        let t = (i as f32) / (n as f32);
        let p = cubic.sample(t);
        acc += point_distance(prev, p);
        prev = p;
        i += 1;
    }
    acc
}

#[cfg(feature = "lyon-native")]
fn tessellate_round_rect_border_mesh() -> Result<(Vec<f32>, Vec<u16>), &'static str> {
    let mut builder = Path::builder();
    builder.add_rounded_rectangle(
        &lyon_tessellation::path::math::Box2D::new(tess_point(0.0, 0.0), tess_point(44.0, 28.0)),
        &BorderRadii::new(6.0),
        lyon_tessellation::path::Winding::Positive,
    );
    let path = builder.build();

    let mut geometry: VertexBuffers<lyon_tessellation::math::Point, u16> = VertexBuffers::new();
    let mut tess = StrokeTessellator::new();
    let opts = StrokeOptions::default().with_line_width(1.0);
    if tess
        .tessellate_path(&path, &opts, &mut simple_builder(&mut geometry))
        .is_err()
    {
        return Err("round-rect stroke tessellation failed");
    }

    let mut verts_xy = Vec::with_capacity(geometry.vertices.len() * 2);
    let mut vi = 0usize;
    while vi < geometry.vertices.len() {
        let p = geometry.vertices[vi];
        verts_xy.push(p.x);
        verts_xy.push(p.y);
        vi += 1;
    }

    Ok((verts_xy, geometry.indices))
}

unsafe extern "C" fn qjs_lyon_demo_shapes(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let out = unsafe { qjs::JS_NewObject(ctx) };
    if out.is_exception() {
        return out;
    }

    #[cfg(feature = "lyon-native")]
    {
        let line = LineSegment {
            from: point(0.0, 0.0),
            to: point(40.0, 30.0),
        };
        let line_len = line.length() as f64;
        let line_mid = line.sample(0.5);
        let line_q1 = line.sample(0.25);
        let line_q3 = line.sample(0.75);
        let (line_l, line_r) = line.split(0.5);

        let quad = QuadraticBezierSegment {
            from: point(0.0, 0.0),
            ctrl: point(20.0, 36.0),
            to: point(44.0, 2.0),
        };
        let quad_q1 = quad.sample(0.25);
        let quad_mid = quad.sample(0.5);
        let quad_q3 = quad.sample(0.75);
        let quad_len = quad.length() as f64;
        let quad_len_approx = approx_quad_length(&quad, 20);
        let quad_base_len = quad.baseline().length() as f64;
        let (quad_l, quad_r) = quad.split(0.5);
        let quad_l_len = quad_l.length() as f64;
        let quad_r_len = quad_r.length() as f64;

        let cubic = CubicBezierSegment {
            from: point(0.0, 0.0),
            ctrl1: point(12.0, 34.0),
            ctrl2: point(38.0, -12.0),
            to: point(60.0, 10.0),
        };
        let cubic_q1 = cubic.sample(0.25);
        let cubic_mid = cubic.sample(0.5);
        let cubic_q3 = cubic.sample(0.75);
        let cubic_base_len = cubic.baseline().length() as f64;
        let cubic_len_approx = approx_cubic_length(&cubic, 32);
        let (cubic_l, cubic_r) = cubic.split(0.5);
        let cubic_l_len_approx = approx_cubic_length(&cubic_l, 16);
        let cubic_r_len_approx = approx_cubic_length(&cubic_r, 16);

        let tri_area = triangle_area((0.0, 0.0), (24.0, 0.0), (10.0, 18.0));
        let tri_signed = triangle_signed_area((0.0, 0.0), (24.0, 0.0), (10.0, 18.0));
        let tri_tess = tessellate_round_rect_border_mesh();

        unsafe { set_prop_str(ctx, out, "ok", js_bool(true)) };
        unsafe { set_prop_str(ctx, out, "lineLength", js_num(ctx, line_len)) };
        unsafe { set_prop_str(ctx, out, "lineMidX", js_num(ctx, line_mid.x as f64)) };
        unsafe { set_prop_str(ctx, out, "lineMidY", js_num(ctx, line_mid.y as f64)) };
        unsafe { set_prop_str(ctx, out, "lineQ1X", js_num(ctx, line_q1.x as f64)) };
        unsafe { set_prop_str(ctx, out, "lineQ1Y", js_num(ctx, line_q1.y as f64)) };
        unsafe { set_prop_str(ctx, out, "lineQ3X", js_num(ctx, line_q3.x as f64)) };
        unsafe { set_prop_str(ctx, out, "lineQ3Y", js_num(ctx, line_q3.y as f64)) };
        unsafe { set_prop_str(ctx, out, "lineLeftLen", js_num(ctx, line_l.length() as f64)) };
        unsafe { set_prop_str(ctx, out, "lineRightLen", js_num(ctx, line_r.length() as f64)) };

        unsafe { set_prop_str(ctx, out, "triangleArea", js_num(ctx, tri_area)) };
        unsafe { set_prop_str(ctx, out, "triangleSignedArea", js_num(ctx, tri_signed)) };
        match tri_tess {
            Ok((verts_xy, idx)) => {
                let verts = verts_xy.len() / 2;
                unsafe { set_prop_str(ctx, out, "triangleTessOk", js_bool(true)) };
                unsafe { set_prop_str(ctx, out, "triangleTessVertices", js_num(ctx, verts as f64)) };
                unsafe { set_prop_str(ctx, out, "triangleTessIndices", js_num(ctx, idx.len() as f64)) };
                unsafe {
                    set_prop_str(
                        ctx,
                        out,
                        "triangleTessTriangles",
                        js_num(ctx, (idx.len() / 3) as f64),
                    )
                };
                let verts_arr = unsafe { js_f32_array(ctx, &verts_xy) };
                unsafe { set_prop_str(ctx, out, "triangleVertices", verts_arr) };
                let idx_arr = unsafe { js_u16_array(ctx, &idx) };
                unsafe { set_prop_str(ctx, out, "triangleIndices", idx_arr) };
            }
            Err(msg) => {
                unsafe { set_prop_str(ctx, out, "triangleTessOk", js_bool(false)) };
                unsafe { set_prop_str(ctx, out, "triangleTessError", js_str(ctx, msg)) };
            }
        }

        unsafe { set_prop_str(ctx, out, "quadLength", js_num(ctx, quad_len)) };
        unsafe { set_prop_str(ctx, out, "quadApproxLength", js_num(ctx, quad_len_approx)) };
        unsafe { set_prop_str(ctx, out, "quadBaselineLen", js_num(ctx, quad_base_len)) };
        unsafe { set_prop_str(ctx, out, "quadMidX", js_num(ctx, quad_mid.x as f64)) };
        unsafe { set_prop_str(ctx, out, "quadMidY", js_num(ctx, quad_mid.y as f64)) };
        unsafe { set_prop_str(ctx, out, "quadQ1X", js_num(ctx, quad_q1.x as f64)) };
        unsafe { set_prop_str(ctx, out, "quadQ1Y", js_num(ctx, quad_q1.y as f64)) };
        unsafe { set_prop_str(ctx, out, "quadQ3X", js_num(ctx, quad_q3.x as f64)) };
        unsafe { set_prop_str(ctx, out, "quadQ3Y", js_num(ctx, quad_q3.y as f64)) };
        unsafe { set_prop_str(ctx, out, "quadLeftLen", js_num(ctx, quad_l_len)) };
        unsafe { set_prop_str(ctx, out, "quadRightLen", js_num(ctx, quad_r_len)) };

        unsafe { set_prop_str(ctx, out, "cubicApproxLength", js_num(ctx, cubic_len_approx)) };
        unsafe { set_prop_str(ctx, out, "cubicBaselineLen", js_num(ctx, cubic_base_len)) };
        unsafe { set_prop_str(ctx, out, "cubicMidX", js_num(ctx, cubic_mid.x as f64)) };
        unsafe { set_prop_str(ctx, out, "cubicMidY", js_num(ctx, cubic_mid.y as f64)) };
        unsafe { set_prop_str(ctx, out, "cubicQ1X", js_num(ctx, cubic_q1.x as f64)) };
        unsafe { set_prop_str(ctx, out, "cubicQ1Y", js_num(ctx, cubic_q1.y as f64)) };
        unsafe { set_prop_str(ctx, out, "cubicQ3X", js_num(ctx, cubic_q3.x as f64)) };
        unsafe { set_prop_str(ctx, out, "cubicQ3Y", js_num(ctx, cubic_q3.y as f64)) };
        unsafe { set_prop_str(ctx, out, "cubicLeftApproxLen", js_num(ctx, cubic_l_len_approx)) };
        unsafe { set_prop_str(ctx, out, "cubicRightApproxLen", js_num(ctx, cubic_r_len_approx)) };
    }

    #[cfg(not(feature = "lyon-native"))]
    {
        unsafe { set_prop_str(ctx, out, "ok", js_bool(false)) };
        unsafe { set_prop_str(ctx, out, "error", js_str(ctx, "lyon backend disabled")) };
    }

    out
}

pub(crate) unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }

    let name = unsafe { CStr::from_ptr(module_name).to_bytes() };
    if name != b"trueos:lyon" && name != b"lyon-native" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn lyon_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> i32 {
        let demo_name = b"demoShapes\0";
        let demo_fn = unsafe {
            qjs::JS_NewCFunction2(
                ctx,
                Some(qjs_lyon_demo_shapes),
                demo_name.as_ptr() as *const c_char,
                0,
                qjs::JS_CFUNC_GENERIC,
                0,
            )
        };
        let _ = unsafe { qjs::JS_SetModuleExport(ctx, m, demo_name.as_ptr() as *const c_char, demo_fn) };

        let avail_name = b"isAvailable\0";
        #[cfg(feature = "lyon-native")]
        let avail = js_bool(true);
        #[cfg(not(feature = "lyon-native"))]
        let avail = js_bool(false);
        let _ = unsafe { qjs::JS_SetModuleExport(ctx, m, avail_name.as_ptr() as *const c_char, avail) };
        0
    }

    let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(lyon_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"demoShapes\0".as_ptr() as *const c_char) };
    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"isAvailable\0".as_ptr() as *const c_char) };
    m
}
