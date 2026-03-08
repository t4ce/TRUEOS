#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_lyon_is_available() -> u32;
    fn trueos_cabi_lyon_demo_mesh_count() -> u32;
    fn trueos_cabi_lyon_demo_mesh_name_len(index: u32) -> usize;
    fn trueos_cabi_lyon_demo_mesh_name_copy(index: u32, out_ptr: *mut u8, out_len: usize) -> usize;
    fn trueos_cabi_lyon_demo_mesh_vertex_count(index: u32) -> u32;
    fn trueos_cabi_lyon_demo_mesh_index_count(index: u32) -> u32;
    fn trueos_cabi_lyon_demo_mesh_copy_vertices(index: u32, out_ptr: *mut f32, out_len: usize)
    -> usize;
    fn trueos_cabi_lyon_demo_mesh_copy_indices(index: u32, out_ptr: *mut u16, out_len: usize)
    -> usize;
}

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
unsafe fn set_prop_str(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &str,
    val: qjs::JSValue,
) {
    let mut keyz = String::from(key);
    keyz.push('\0');
    let _ = unsafe { qjs::JS_SetPropertyStr(ctx, obj, keyz.as_ptr() as *const c_char, val) };
}

struct DemoMeshData {
    name: String,
    vertices: Vec<f32>,
    indices: Vec<u16>,
}

fn read_mesh_name(index: u32) -> Option<String> {
    let name_len = unsafe { trueos_cabi_lyon_demo_mesh_name_len(index) };
    if name_len == 0 {
        return None;
    }
    let mut buf = vec![0u8; name_len];
    let wrote = unsafe { trueos_cabi_lyon_demo_mesh_name_copy(index, buf.as_mut_ptr(), buf.len()) };
    if wrote == 0 {
        return None;
    }
    buf.truncate(wrote);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn read_demo_mesh(index: u32) -> Option<DemoMeshData> {
    let vcount = unsafe { trueos_cabi_lyon_demo_mesh_vertex_count(index) } as usize;
    let icount = unsafe { trueos_cabi_lyon_demo_mesh_index_count(index) } as usize;
    if vcount == 0 || icount < 3 {
        return None;
    }

    let mut vertices = vec![0.0f32; vcount.saturating_mul(2)];
    let mut indices = vec![0u16; icount];

    let v_written = unsafe {
        trueos_cabi_lyon_demo_mesh_copy_vertices(index, vertices.as_mut_ptr(), vertices.len())
    };
    let i_written = unsafe {
        trueos_cabi_lyon_demo_mesh_copy_indices(index, indices.as_mut_ptr(), indices.len())
    };
    if v_written == 0 || i_written < 3 {
        return None;
    }

    vertices.truncate(v_written);
    indices.truncate(i_written);

    let name = read_mesh_name(index).unwrap_or_else(|| String::from("mesh"));
    Some(DemoMeshData {
        name,
        vertices,
        indices,
    })
}

fn read_demo_meshes() -> Vec<DemoMeshData> {
    let count = unsafe { trueos_cabi_lyon_demo_mesh_count() } as usize;
    let mut out = Vec::with_capacity(count);
    let mut i = 0u32;
    while (i as usize) < count {
        if let Some(m) = read_demo_mesh(i) {
            out.push(m);
        }
        i += 1;
    }
    out
}

#[inline]
fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    libm::sqrt(dx * dx + dy * dy)
}

#[inline]
fn quad_sample(t: f64, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) -> (f64, f64) {
    let u = 1.0 - t;
    (
        u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
        u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
    )
}

#[inline]
fn cubic_sample(
    t: f64,
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
) -> (f64, f64) {
    let u = 1.0 - t;
    let u2 = u * u;
    let t2 = t * t;
    (
        u2 * u * p0.0 + 3.0 * u2 * t * p1.0 + 3.0 * u * t2 * p2.0 + t2 * t * p3.0,
        u2 * u * p0.1 + 3.0 * u2 * t * p1.1 + 3.0 * u * t2 * p2.1 + t2 * t * p3.1,
    )
}

fn approx_len(mut sample: impl FnMut(f64) -> (f64, f64), steps: u32) -> f64 {
    let n = if steps < 2 { 2 } else { steps };
    let mut acc = 0.0;
    let mut prev = sample(0.0);
    let mut i = 1u32;
    while i <= n {
        let t = (i as f64) / (n as f64);
        let p = sample(t);
        acc += dist(prev, p);
        prev = p;
        i += 1;
    }
    acc
}

unsafe extern "C" fn qjs_lyon_demo_meshes(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let out = unsafe { qjs::JS_NewObject(ctx) };
    if out.is_exception() {
        return out;
    }

    let available = unsafe { trueos_cabi_lyon_is_available() != 0 };
    if !available {
        unsafe { set_prop_str(ctx, out, "ok", js_bool(false)) };
        unsafe { set_prop_str(ctx, out, "error", js_str(ctx, "lyon backend disabled")) };
        return out;
    }

    let meshes = read_demo_meshes();
    if meshes.is_empty() {
        unsafe { set_prop_str(ctx, out, "ok", js_bool(false)) };
        unsafe { set_prop_str(ctx, out, "error", js_str(ctx, "no demo meshes")) };
        return out;
    }

    let arr = unsafe { qjs::JS_NewArray(ctx) };
    if arr.is_exception() {
        unsafe { set_prop_str(ctx, out, "ok", js_bool(false)) };
        unsafe { set_prop_str(ctx, out, "error", js_str(ctx, "mesh-array alloc failed")) };
        return out;
    }

    let mut i = 0u32;
    while (i as usize) < meshes.len() {
        let msrc = &meshes[i as usize];
        let m = unsafe { qjs::JS_NewObject(ctx) };
        if !m.is_exception() {
            unsafe {
                set_prop_str(ctx, m, "name", js_str(ctx, msrc.name.as_str()));
                set_prop_str(ctx, m, "vertices", js_f32_array(ctx, msrc.vertices.as_slice()));
                set_prop_str(ctx, m, "indices", js_u16_array(ctx, msrc.indices.as_slice()));
                set_prop_str(
                    ctx,
                    m,
                    "vertexCount",
                    js_num(ctx, (msrc.vertices.len() / 2) as f64),
                );
                set_prop_str(ctx, m, "indexCount", js_num(ctx, msrc.indices.len() as f64));
                set_prop_str(
                    ctx,
                    m,
                    "triangleCount",
                    js_num(ctx, (msrc.indices.len() / 3) as f64),
                );
            }
            let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, i, m) };
        }
        i += 1;
    }

    unsafe { set_prop_str(ctx, out, "ok", js_bool(true)) };
    unsafe { set_prop_str(ctx, out, "meshCount", js_num(ctx, meshes.len() as f64)) };
    unsafe { set_prop_str(ctx, out, "meshes", arr) };
    out
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

    let available = unsafe { trueos_cabi_lyon_is_available() != 0 };
    if !available {
        unsafe { set_prop_str(ctx, out, "ok", js_bool(false)) };
        unsafe { set_prop_str(ctx, out, "error", js_str(ctx, "lyon backend disabled")) };
        return out;
    }

    let p0 = (0.0, 0.0);
    let p1 = (40.0, 30.0);
    let line_len = dist(p0, p1);
    let line_q1 = (10.0, 7.5);
    let line_mid = (20.0, 15.0);
    let line_q3 = (30.0, 22.5);

    let q0 = (0.0, 0.0);
    let q1 = (20.0, 36.0);
    let q2 = (44.0, 2.0);
    let quad_q1 = quad_sample(0.25, q0, q1, q2);
    let quad_mid = quad_sample(0.5, q0, q1, q2);
    let quad_q3 = quad_sample(0.75, q0, q1, q2);
    let quad_len = approx_len(|t| quad_sample(t, q0, q1, q2), 32);
    let quad_base = dist(q0, q2);

    let c0 = (0.0, 0.0);
    let c1 = (12.0, 34.0);
    let c2 = (38.0, -12.0);
    let c3 = (60.0, 10.0);
    let cubic_q1 = cubic_sample(0.25, c0, c1, c2, c3);
    let cubic_mid = cubic_sample(0.5, c0, c1, c2, c3);
    let cubic_q3 = cubic_sample(0.75, c0, c1, c2, c3);
    let cubic_len = approx_len(|t| cubic_sample(t, c0, c1, c2, c3), 48);
    let cubic_base = dist(c0, c3);

    let tri_area = 216.0f64;
    let tri_signed = 216.0f64;

    unsafe { set_prop_str(ctx, out, "ok", js_bool(true)) };
    unsafe { set_prop_str(ctx, out, "lineLength", js_num(ctx, line_len)) };
    unsafe { set_prop_str(ctx, out, "lineMidX", js_num(ctx, line_mid.0)) };
    unsafe { set_prop_str(ctx, out, "lineMidY", js_num(ctx, line_mid.1)) };
    unsafe { set_prop_str(ctx, out, "lineQ1X", js_num(ctx, line_q1.0)) };
    unsafe { set_prop_str(ctx, out, "lineQ1Y", js_num(ctx, line_q1.1)) };
    unsafe { set_prop_str(ctx, out, "lineQ3X", js_num(ctx, line_q3.0)) };
    unsafe { set_prop_str(ctx, out, "lineQ3Y", js_num(ctx, line_q3.1)) };
    unsafe { set_prop_str(ctx, out, "lineLeftLen", js_num(ctx, line_len * 0.5)) };
    unsafe { set_prop_str(ctx, out, "lineRightLen", js_num(ctx, line_len * 0.5)) };

    unsafe { set_prop_str(ctx, out, "quadLength", js_num(ctx, quad_len)) };
    unsafe { set_prop_str(ctx, out, "quadApproxLength", js_num(ctx, quad_len)) };
    unsafe { set_prop_str(ctx, out, "quadBaselineLen", js_num(ctx, quad_base)) };
    unsafe { set_prop_str(ctx, out, "quadMidX", js_num(ctx, quad_mid.0)) };
    unsafe { set_prop_str(ctx, out, "quadMidY", js_num(ctx, quad_mid.1)) };
    unsafe { set_prop_str(ctx, out, "quadQ1X", js_num(ctx, quad_q1.0)) };
    unsafe { set_prop_str(ctx, out, "quadQ1Y", js_num(ctx, quad_q1.1)) };
    unsafe { set_prop_str(ctx, out, "quadQ3X", js_num(ctx, quad_q3.0)) };
    unsafe { set_prop_str(ctx, out, "quadQ3Y", js_num(ctx, quad_q3.1)) };
    unsafe { set_prop_str(ctx, out, "quadLeftLen", js_num(ctx, quad_len * 0.5)) };
    unsafe { set_prop_str(ctx, out, "quadRightLen", js_num(ctx, quad_len * 0.5)) };

    unsafe { set_prop_str(ctx, out, "cubicApproxLength", js_num(ctx, cubic_len)) };
    unsafe { set_prop_str(ctx, out, "cubicBaselineLen", js_num(ctx, cubic_base)) };
    unsafe { set_prop_str(ctx, out, "cubicMidX", js_num(ctx, cubic_mid.0)) };
    unsafe { set_prop_str(ctx, out, "cubicMidY", js_num(ctx, cubic_mid.1)) };
    unsafe { set_prop_str(ctx, out, "cubicQ1X", js_num(ctx, cubic_q1.0)) };
    unsafe { set_prop_str(ctx, out, "cubicQ1Y", js_num(ctx, cubic_q1.1)) };
    unsafe { set_prop_str(ctx, out, "cubicQ3X", js_num(ctx, cubic_q3.0)) };
    unsafe { set_prop_str(ctx, out, "cubicQ3Y", js_num(ctx, cubic_q3.1)) };
    unsafe { set_prop_str(ctx, out, "cubicLeftApproxLen", js_num(ctx, cubic_len * 0.5)) };
    unsafe { set_prop_str(ctx, out, "cubicRightApproxLen", js_num(ctx, cubic_len * 0.5)) };

    unsafe { set_prop_str(ctx, out, "triangleArea", js_num(ctx, tri_area)) };
    unsafe { set_prop_str(ctx, out, "triangleSignedArea", js_num(ctx, tri_signed)) };

    let meshes = read_demo_meshes();
    if let Some(first) = meshes.first() {
        unsafe { set_prop_str(ctx, out, "triangleTessOk", js_bool(true)) };
        unsafe {
            set_prop_str(
                ctx,
                out,
                "triangleTessVertices",
                js_num(ctx, (first.vertices.len() / 2) as f64),
            )
        };
        unsafe {
            set_prop_str(
                ctx,
                out,
                "triangleTessIndices",
                js_num(ctx, first.indices.len() as f64),
            )
        };
        unsafe {
            set_prop_str(
                ctx,
                out,
                "triangleTessTriangles",
                js_num(ctx, (first.indices.len() / 3) as f64),
            )
        };
        let verts_arr = unsafe { js_f32_array(ctx, first.vertices.as_slice()) };
        unsafe { set_prop_str(ctx, out, "triangleVertices", verts_arr) };
        let idx_arr = unsafe { js_u16_array(ctx, first.indices.as_slice()) };
        unsafe { set_prop_str(ctx, out, "triangleIndices", idx_arr) };

        let arr = unsafe { qjs::JS_NewArray(ctx) };
        if !arr.is_exception() {
            let mut i = 0u32;
            while (i as usize) < meshes.len() {
                let msrc = &meshes[i as usize];
                let m = unsafe { qjs::JS_NewObject(ctx) };
                if !m.is_exception() {
                    unsafe {
                        set_prop_str(ctx, m, "name", js_str(ctx, msrc.name.as_str()));
                        set_prop_str(ctx, m, "vertices", js_f32_array(ctx, msrc.vertices.as_slice()));
                        set_prop_str(ctx, m, "indices", js_u16_array(ctx, msrc.indices.as_slice()));
                        set_prop_str(ctx, m, "vertexCount", js_num(ctx, (msrc.vertices.len() / 2) as f64));
                        set_prop_str(ctx, m, "indexCount", js_num(ctx, msrc.indices.len() as f64));
                        set_prop_str(ctx, m, "triangleCount", js_num(ctx, (msrc.indices.len() / 3) as f64));
                    }
                    let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, i, m) };
                }
                i += 1;
            }
            unsafe { set_prop_str(ctx, out, "demoMeshesOk", js_bool(true)) };
            unsafe { set_prop_str(ctx, out, "demoMeshCount", js_num(ctx, meshes.len() as f64)) };
            unsafe { set_prop_str(ctx, out, "demoMeshes", arr) };
        }
    } else {
        unsafe { set_prop_str(ctx, out, "triangleTessOk", js_bool(false)) };
        unsafe { set_prop_str(ctx, out, "triangleTessError", js_str(ctx, "no demo meshes")) };
        unsafe { set_prop_str(ctx, out, "demoMeshesOk", js_bool(false)) };
        unsafe { set_prop_str(ctx, out, "demoMeshesError", js_str(ctx, "no demo meshes")) };
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
        let _ = unsafe {
            qjs::JS_SetModuleExport(ctx, m, demo_name.as_ptr() as *const c_char, demo_fn)
        };

        let meshes_name = b"demoMeshes\0";
        let meshes_fn = unsafe {
            qjs::JS_NewCFunction2(
                ctx,
                Some(qjs_lyon_demo_meshes),
                meshes_name.as_ptr() as *const c_char,
                0,
                qjs::JS_CFUNC_GENERIC,
                0,
            )
        };
        let _ = unsafe {
            qjs::JS_SetModuleExport(ctx, m, meshes_name.as_ptr() as *const c_char, meshes_fn)
        };

        let avail_name = b"isAvailable\0";
        let avail = js_bool(unsafe { trueos_cabi_lyon_is_available() != 0 });
        let _ = unsafe {
            qjs::JS_SetModuleExport(ctx, m, avail_name.as_ptr() as *const c_char, avail)
        };
        0
    }

    let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(lyon_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"demoShapes\0".as_ptr() as *const c_char) };
    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"demoMeshes\0".as_ptr() as *const c_char) };
    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"isAvailable\0".as_ptr() as *const c_char) };
    m
}
