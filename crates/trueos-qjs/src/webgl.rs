extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::{c_char, c_int, CStr};
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use crate as qjs;
use crate::webgl_texture::WebGlTextureState;

extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    // NOTE: The WebGL shim targets the stable gfx layer (trueos_gfx_core), not a concrete GPU.
    // Backends (software / virtio-gpu / Xe) sit behind this ABI.
    fn trueos_cabi_gfx_draw_rgb_triangles(clear_rgb: u32, vtx_ptr: *const u8, vtx_len: usize) -> i32;
}

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

fn log_usize_dec(v: usize) {
    if v == 0 {
        log_str("0");
        return;
    }
    let mut n = v as u64;
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n != 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    log_bytes(&buf[i..]);
}

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

#[inline]
fn js_null() -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: 0 },
        tag: qjs::JS_TAG_NULL,
    }
}

#[inline]
fn js_bool(v: bool) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: if v { 1 } else { 0 } },
        tag: qjs::JS_TAG_BOOL,
    }
}
// --- Minimal WebGL-ish shim state ---

static WEBGL_NEXT_ID: AtomicU32 = AtomicU32::new(1);
static WEBGL_DID_LOG_DRAW: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_UNIFORM_LOOKUPS: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_UNIFORM_UPLOADS: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_DRAW_MODE: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_DRAW_DROPS: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_GET_CONTEXT: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_CREATE_HANDLE: AtomicU32 = AtomicU32::new(0);

#[inline]
fn webgl_log_draw_drop(where_: &str, why: &str) {
    if WEBGL_LOG_DRAW_DROPS.fetch_add(1, Ordering::Relaxed) < 24 {
        log_str("qjs-webgl: drop ");
        log_str(where_);
        log_str(" reason=");
        log_str(why);
        log_str("\n");
    }
}

#[derive(Clone, Copy, Default)]
struct WebGlVertexAttrib {
    enabled: bool,
    size: i32,
    ty: u32,
    normalized: bool,
    stride: i32,
    offset: usize,
    buffer: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WebGlUniformKind {
    Other,
    TranslationMatrix,
    ProjectionMatrix,
}

#[derive(Clone)]
struct WebGlActiveAttrib {
    name: Vec<u8>,
    gl_type: u32,
    size: i32,
}

#[derive(Clone, Default)]
struct WebGlVaoState {
    element_array_buffer: u32,
    attribs: BTreeMap<u32, WebGlVertexAttrib>,
}

struct WebGlState {
    array_buffer: u32,
    element_array_buffer: u32,
    buffers: BTreeMap<u32, Vec<u8>>,
    attribs: BTreeMap<u32, WebGlVertexAttrib>,
    attrib_name_to_loc: BTreeMap<Vec<u8>, u32>,
    attrib_loc_to_name: BTreeMap<u32, Vec<u8>>,
    next_attrib_loc: u32,
    uniform_locs: BTreeMap<u32, WebGlUniformKind>,
    uniform_name_to_loc: BTreeMap<Vec<u8>, u32>,
    next_uniform_loc: u32,
    shader_types: BTreeMap<u32, u32>,
    shader_sources: BTreeMap<u32, Vec<u8>>,
    program_shaders: BTreeMap<u32, Vec<u32>>,
    program_active_attribs: BTreeMap<u32, Vec<WebGlActiveAttrib>>,
    translation_matrix: [f32; 9],
    projection_matrix: [f32; 9],
    has_translation_matrix: bool,
    has_projection_matrix: bool,
    clear_rgb: u32,
    viewport_w: i32,
    viewport_h: i32,
    enabled_blend: bool,
    enabled_cull_face: bool,
    enabled_depth_test: bool,
    enabled_scissor_test: bool,
    front_face_mode: u32,
    cull_face_mode: u32,
    blend_src_rgb: u32,
    blend_dst_rgb: u32,
    blend_src_alpha: u32,
    blend_dst_alpha: u32,
    blend_eq_rgb: u32,
    blend_eq_alpha: u32,
    current_vao: u32,
    vao0: WebGlVaoState,
    vaos: BTreeMap<u32, WebGlVaoState>,
    textures: WebGlTextureState,
}

static WEBGL_STATE: Mutex<WebGlState> = Mutex::new(WebGlState {
    array_buffer: 0,
    element_array_buffer: 0,
    buffers: BTreeMap::new(),
    attribs: BTreeMap::new(),
    attrib_name_to_loc: BTreeMap::new(),
    attrib_loc_to_name: BTreeMap::new(),
    next_attrib_loc: 0,
    uniform_locs: BTreeMap::new(),
    uniform_name_to_loc: BTreeMap::new(),
    next_uniform_loc: 1,
    shader_types: BTreeMap::new(),
    shader_sources: BTreeMap::new(),
    program_shaders: BTreeMap::new(),
    program_active_attribs: BTreeMap::new(),
    translation_matrix: [
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
    ],
    projection_matrix: [
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
    ],
    has_translation_matrix: false,
    has_projection_matrix: false,
    // TRUEOS default console blue.
    clear_rgb: 0x00_08_18_30,
    viewport_w: 0,
    viewport_h: 0,
    enabled_blend: false,
    enabled_cull_face: false,
    enabled_depth_test: false,
    enabled_scissor_test: false,
    // WebGL defaults: CCW front faces, cull back faces.
    front_face_mode: 0x0901, // CCW
    cull_face_mode: 0x0405,  // BACK
    blend_src_rgb: 1, // ONE
    blend_dst_rgb: 0, // ZERO
    blend_src_alpha: 1, // ONE
    blend_dst_alpha: 0, // ZERO
    blend_eq_rgb: 0x8006, // FUNC_ADD
    blend_eq_alpha: 0x8006, // FUNC_ADD
    current_vao: 0,
    vao0: WebGlVaoState {
        element_array_buffer: 0,
        attribs: BTreeMap::new(),
    },
    vaos: BTreeMap::new(),
    textures: WebGlTextureState {
        active_unit: 0,
        unpack_alignment: 4,
        bound_tex2d_by_unit: BTreeMap::new(),
        params: BTreeMap::new(),
        images: BTreeMap::new(),
    },
});

fn ascii_lower(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        out.push(if b'A' <= b && b <= b'Z' { b + 32 } else { b });
    }
    out
}

fn classify_attrib_name(name: &[u8]) -> Option<u32> {
    let lower = ascii_lower(name);
    if lower.windows(b"position".len()).any(|w| w == b"position") {
        Some(0)
    } else if lower.windows(b"color".len()).any(|w| w == b"color") {
        Some(1)
    } else if lower.windows(b"texcoord".len()).any(|w| w == b"texcoord")
        || lower.windows(b"texturecoord".len()).any(|w| w == b"texturecoord")
        || lower.windows(b"uv".len()).any(|w| w == b"uv")
    {
        Some(2)
    } else {
        None
    }
}

fn alloc_attrib_location(st: &mut WebGlState, name: &[u8]) -> u32 {
    if let Some(&loc) = st.attrib_name_to_loc.get(name) {
        return loc;
    }

    if let Some(loc) = classify_attrib_name(name) {
        let ok = match st.attrib_loc_to_name.get(&loc) {
            Some(existing) => existing.as_slice() == name,
            None => true,
        };
        if ok {
            let key = name.to_vec();
            st.attrib_name_to_loc.insert(key.clone(), loc);
            st.attrib_loc_to_name.insert(loc, key);
            st.next_attrib_loc = st.next_attrib_loc.max(loc.saturating_add(1));
            return loc;
        }
    }

    let mut loc = st.next_attrib_loc.max(3);
    while st.attrib_loc_to_name.contains_key(&loc) {
        loc = loc.saturating_add(1);
    }
    let key = name.to_vec();
    st.attrib_name_to_loc.insert(key.clone(), loc);
    st.attrib_loc_to_name.insert(loc, key);
    st.next_attrib_loc = loc.saturating_add(1);
    loc
}

fn parse_glsl_type_token(ty: &str) -> Option<u32> {
    match ty {
        "float" => Some(0x1406),
        "vec2" => Some(0x8B50),
        "vec3" => Some(0x8B51),
        "vec4" => Some(0x8B52),
        "int" => Some(0x1404),
        "ivec2" => Some(0x8B53),
        "ivec3" => Some(0x8B54),
        "ivec4" => Some(0x8B55),
        "bool" => Some(0x8B56),
        "bvec2" => Some(0x8B57),
        "bvec3" => Some(0x8B58),
        "bvec4" => Some(0x8B59),
        "mat2" => Some(0x8B5A),
        "mat3" => Some(0x8B5B),
        "mat4" => Some(0x8B5C),
        _ => None,
    }
}

fn parse_glsl_active_attribs(src: &[u8]) -> Vec<WebGlActiveAttrib> {
    let mut out = Vec::new();
    let Ok(text) = core::str::from_utf8(src) else {
        return out;
    };

    for raw_line in text.lines() {
        let line = raw_line.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let mut words = line.split_whitespace();
        let Some(qual) = words.next() else {
            continue;
        };
        if qual != "attribute" && qual != "in" {
            continue;
        }
        let Some(ty_tok) = words.next() else {
            continue;
        };
        let Some(name_tok) = words.next() else {
            continue;
        };

        let Some(gl_type) = parse_glsl_type_token(ty_tok) else {
            continue;
        };

        let mut name = name_tok.trim_end_matches(';');
        if let Some(i) = name.find('[') {
            name = &name[..i];
        }
        if name.is_empty() {
            continue;
        }

        let name_bytes = name.as_bytes().to_vec();
        if out.iter().any(|a| a.name == name_bytes) {
            continue;
        }
        out.push(WebGlActiveAttrib {
            name: name_bytes,
            gl_type,
            size: 1,
        });
    }
    out
}

fn refresh_program_active_attribs(st: &mut WebGlState, program_id: u32) {
    let mut out = Vec::new();
    if let Some(shaders) = st.program_shaders.get(&program_id).cloned() {
        for shader_id in shaders {
            let ty = st.shader_types.get(&shader_id).copied().unwrap_or(0);
            // VERTEX_SHADER only.
            if ty != 0x8B31 {
                continue;
            }
            let src = st.shader_sources.get(&shader_id).cloned().unwrap_or_default();
            for a in parse_glsl_active_attribs(src.as_slice()) {
                if !out.iter().any(|x: &WebGlActiveAttrib| x.name == a.name) {
                    out.push(a);
                }
            }
        }
    }
    for a in out.iter() {
        let _ = alloc_attrib_location(st, a.name.as_slice());
    }
    st.program_active_attribs.insert(program_id, out);
}

fn webgl_save_current_vao_state(st: &mut WebGlState) {
    let saved = WebGlVaoState {
        element_array_buffer: st.element_array_buffer,
        attribs: st.attribs.clone(),
    };
    if st.current_vao == 0 {
        st.vao0 = saved;
    } else {
        st.vaos.insert(st.current_vao, saved);
    }
}

fn webgl_load_vao_state(st: &mut WebGlState, vao_id: u32) {
    let state = if vao_id == 0 {
        st.vao0.clone()
    } else {
        st.vaos.get(&vao_id).cloned().unwrap_or_default()
    };
    st.current_vao = vao_id;
    st.element_array_buffer = state.element_array_buffer;
    st.attribs = state.attribs;
}

pub(crate) unsafe fn ensure_global_trueos_webgl_singleton(
    ctx: *mut qjs::JSContext,
    global: qjs::JSValue,
) -> qjs::JSValue {
    let key = b"__trueos_gl\0";
    let existing = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    if !existing.is_exception() && existing.tag != qjs::JS_TAG_UNDEFINED {
        return existing;
    }
    qjs::js_free_value(ctx, existing);

    // --- WebGL shim functions (minimal) ---
    // These implement just enough flow to bridge a triangle/rect draw into the kernel gfx layer.

    unsafe extern "C" fn gl_noop(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_return_null(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        js_null()
    }

    unsafe extern "C" fn gl_return_true(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue {
            u: qjs::JSValueUnion { int32: 1 },
            tag: qjs::JS_TAG_BOOL,
        }
    }

    unsafe extern "C" fn gl_return_false(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue {
            u: qjs::JSValueUnion { int32: 0 },
            tag: qjs::JS_TAG_BOOL,
        }
    }

    unsafe extern "C" fn gl_create_handle(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let id = WEBGL_NEXT_ID.fetch_add(1, Ordering::Relaxed);
        {
            let mut st = WEBGL_STATE.lock();
            st.buffers.entry(id).or_insert_with(Vec::new);
        }
        if WEBGL_LOG_CREATE_HANDLE.fetch_add(1, Ordering::Relaxed) < 12 {
            log_str("qjs-webgl: createHandle id=");
            log_usize_dec(id as usize);
            log_str("\n");
        }
        js_int32(id as i32)
    }

    unsafe extern "C" fn gl_create_shader(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let out = gl_create_handle(ctx, this_val, 0, core::ptr::null());
        if out.tag != qjs::JS_TAG_INT {
            return out;
        }
        if argv.is_null() || argc < 1 {
            return out;
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut ty_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut ty_f as *mut f64, args[0]) != 0 {
            return out;
        }
        let shader_ty = (ty_f as i32).max(0) as u32;
        let shader_id = unsafe { out.u.int32 }.max(0) as u32;
        if shader_id != 0 {
            let mut st = WEBGL_STATE.lock();
            st.shader_types.insert(shader_id, shader_ty);
        }
        out
    }

    unsafe extern "C" fn gl_shader_source(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut shader_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut shader_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let shader_id = (shader_f as i32).max(0) as u32;
        if shader_id == 0 {
            return qjs::JSValue::undefined();
        }
        let cstr = qjs::js_to_cstring(ctx, args[1]);
        if cstr.is_null() {
            return qjs::JSValue::undefined();
        }
        let src = CStr::from_ptr(cstr).to_bytes().to_vec();
        qjs::JS_FreeCString(ctx, cstr);
        let mut st = WEBGL_STATE.lock();
        st.shader_sources.insert(shader_id, src);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_attach_shader(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut program_f: f64 = 0.0;
        let mut shader_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut program_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut shader_f as *mut f64, args[1]);
        let program_id = (program_f as i32).max(0) as u32;
        let shader_id = (shader_f as i32).max(0) as u32;
        if program_id == 0 || shader_id == 0 {
            return qjs::JSValue::undefined();
        }
        let mut st = WEBGL_STATE.lock();
        let v = st.program_shaders.entry(program_id).or_insert_with(Vec::new);
        if !v.iter().any(|x| *x == shader_id) {
            v.push(shader_id);
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_detach_shader(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut program_f: f64 = 0.0;
        let mut shader_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut program_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut shader_f as *mut f64, args[1]);
        let program_id = (program_f as i32).max(0) as u32;
        let shader_id = (shader_f as i32).max(0) as u32;
        if program_id == 0 || shader_id == 0 {
            return qjs::JSValue::undefined();
        }
        let mut st = WEBGL_STATE.lock();
        if let Some(v) = st.program_shaders.get_mut(&program_id) {
            v.retain(|x| *x != shader_id);
        }
        refresh_program_active_attribs(&mut st, program_id);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_link_program(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut program_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut program_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let program_id = (program_f as i32).max(0) as u32;
        if program_id == 0 {
            return qjs::JSValue::undefined();
        }
        let mut st = WEBGL_STATE.lock();
        refresh_program_active_attribs(&mut st, program_id);
        qjs::JSValue::undefined()
    }

fn classify_uniform_name(name: &[u8]) -> WebGlUniformKind {
    let raw = if let Some(base) = name.strip_suffix(b"[0]") {
        base
    } else {
        name
    };
    let lower = ascii_lower(raw);
    if lower
        .windows(b"translationmatrix".len())
        .any(|w| w == b"translationmatrix")
    {
        WebGlUniformKind::TranslationMatrix
    } else if lower
        .windows(b"projectionmatrix".len())
        .any(|w| w == b"projectionmatrix")
    {
        WebGlUniformKind::ProjectionMatrix
    } else {
        WebGlUniformKind::Other
    }
}

    fn mat3_mul_vec3(m: &[f32; 9], x: f32, y: f32, z: f32) -> (f32, f32, f32) {
        // Column-major mat3, matching GLSL/WebGL uniform layout.
        let ox = m[0] * x + m[3] * y + m[6] * z;
        let oy = m[1] * x + m[4] * y + m[7] * z;
        let oz = m[2] * x + m[5] * y + m[8] * z;
        (ox, oy, oz)
    }

    fn mat3_transpose(m: [f32; 9]) -> [f32; 9] {
        [
            m[0], m[3], m[6],
            m[1], m[4], m[7],
            m[2], m[5], m[8],
        ]
    }

    fn mat4_to_mat3_affine_2d(m: [f32; 16]) -> [f32; 9] {
        // Column-major 4x4 for vec4(x, y, 0, 1) -> vec2.
        [
            m[0], m[1], 0.0,
            m[4], m[5], 0.0,
            m[12], m[13], 1.0,
        ]
    }

    unsafe extern "C" fn gl_get_uniform_location(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return js_null();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let cstr = qjs::js_to_cstring(ctx, args[1]);
        if cstr.is_null() {
            return js_null();
        }
        let name = CStr::from_ptr(cstr).to_bytes().to_vec();
        qjs::JS_FreeCString(ctx, cstr);
        if name.is_empty() {
            return js_null();
        }

        let mut st = WEBGL_STATE.lock();
        if let Some(&loc) = st.uniform_name_to_loc.get(&name) {
            return js_int32(loc as i32);
        }
        let loc = st.next_uniform_loc.max(1);
        st.next_uniform_loc = loc.saturating_add(1);
        st.uniform_name_to_loc.insert(name.clone(), loc);
        let kind = classify_uniform_name(name.as_slice());
        st.uniform_locs.insert(loc, kind);
        if WEBGL_LOG_UNIFORM_LOOKUPS.fetch_add(1, Ordering::Relaxed) < 8 {
            log_str("qjs-webgl: getUniformLocation name=");
            log_bytes(name.as_slice());
            log_str(" loc=");
            log_usize_dec(loc as usize);
            log_str(" kind=");
            match kind {
                WebGlUniformKind::TranslationMatrix => log_str("translation"),
                WebGlUniformKind::ProjectionMatrix => log_str("projection"),
                WebGlUniformKind::Other => log_str("other"),
            }
            log_str("\n");
        }
        js_int32(loc as i32)
    }

    unsafe extern "C" fn gl_bind_attrib_location(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut idx_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut idx_f as *mut f64, args[1]) != 0 {
            return qjs::JSValue::undefined();
        }
        let loc = (idx_f as i32).max(0) as u32;
        let cstr = qjs::js_to_cstring(ctx, args[2]);
        if cstr.is_null() {
            return qjs::JSValue::undefined();
        }
        let name = CStr::from_ptr(cstr).to_bytes().to_vec();
        qjs::JS_FreeCString(ctx, cstr);
        if name.is_empty() {
            return qjs::JSValue::undefined();
        }
        let mut st = WEBGL_STATE.lock();
        st.attrib_name_to_loc.insert(name.clone(), loc);
        st.attrib_loc_to_name.insert(loc, name);
        st.next_attrib_loc = st.next_attrib_loc.max(loc.saturating_add(1));
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_get_attrib_location(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return js_int32(-1);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut program_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut program_f as *mut f64, args[0]);
        let program_id = (program_f as i32).max(0) as u32;
        let cstr = qjs::js_to_cstring(ctx, args[1]);
        if cstr.is_null() {
            return js_int32(-1);
        }
        let name = CStr::from_ptr(cstr).to_bytes().to_vec();
        qjs::JS_FreeCString(ctx, cstr);
        if name.is_empty() {
            return js_int32(-1);
        }
        let mut st = WEBGL_STATE.lock();
        if program_id != 0 && !st.program_active_attribs.contains_key(&program_id) {
            refresh_program_active_attribs(&mut st, program_id);
        }
        if program_id != 0 {
            let known = st
                .program_active_attribs
                .get(&program_id)
                .map(|v| v.iter().any(|a| a.name == name))
                .unwrap_or(false);
            if !known {
                return js_int32(-1);
            }
        }
        let loc = alloc_attrib_location(&mut st, name.as_slice());
        js_int32(loc as i32)
    }

    unsafe extern "C" fn gl_uniform_matrix3fv(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut loc_f: f64 = 0.0;
        let mut transpose_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut loc_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut transpose_f as *mut f64, args[1]);
        let loc = (loc_f as i32).max(0) as u32;
        if loc == 0 {
            return qjs::JSValue::undefined();
        }

        let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[2]) else {
            return qjs::JSValue::undefined();
        };
        let bytes = core::slice::from_raw_parts(ptr, len);
        if bytes.len() < 9 * 4 {
            return qjs::JSValue::undefined();
        }

        let mut mat = [0.0f32; 9];
        for (i, slot) in mat.iter_mut().enumerate() {
            let Some(v) = read_f32_le(bytes, i * 4) else {
                return qjs::JSValue::undefined();
            };
            *slot = v;
        }
        if transpose_f != 0.0 {
            mat = mat3_transpose(mat);
        }

        let mut st = WEBGL_STATE.lock();
        match st.uniform_locs.get(&loc).copied().unwrap_or(WebGlUniformKind::Other) {
            WebGlUniformKind::TranslationMatrix => {
                st.translation_matrix = mat;
                st.has_translation_matrix = true;
            }
            WebGlUniformKind::ProjectionMatrix => {
                st.projection_matrix = mat;
                st.has_projection_matrix = true;
            }
            WebGlUniformKind::Other => {}
        }
        if WEBGL_LOG_UNIFORM_UPLOADS.fetch_add(1, Ordering::Relaxed) < 16 {
            log_str("qjs-webgl: uniformMatrix3fv loc=");
            log_usize_dec(loc as usize);
            log_str(" transpose=");
            log_usize_dec((transpose_f != 0.0) as usize);
            log_str("\n");
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_uniform_matrix4fv(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut loc_f: f64 = 0.0;
        let mut transpose_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut loc_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut transpose_f as *mut f64, args[1]);
        let loc = (loc_f as i32).max(0) as u32;
        if loc == 0 {
            return qjs::JSValue::undefined();
        }

        let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[2]) else {
            return qjs::JSValue::undefined();
        };
        let bytes = core::slice::from_raw_parts(ptr, len);
        if bytes.len() < 16 * 4 {
            return qjs::JSValue::undefined();
        }

        let mut mat4 = [0.0f32; 16];
        for (i, slot) in mat4.iter_mut().enumerate() {
            let Some(v) = read_f32_le(bytes, i * 4) else {
                return qjs::JSValue::undefined();
            };
            *slot = v;
        }
        let mut mat3 = mat4_to_mat3_affine_2d(mat4);
        if transpose_f != 0.0 {
            mat3 = mat3_transpose(mat3);
        }

        let mut st = WEBGL_STATE.lock();
        match st.uniform_locs.get(&loc).copied().unwrap_or(WebGlUniformKind::Other) {
            WebGlUniformKind::TranslationMatrix => {
                st.translation_matrix = mat3;
                st.has_translation_matrix = true;
            }
            WebGlUniformKind::ProjectionMatrix => {
                st.projection_matrix = mat3;
                st.has_projection_matrix = true;
            }
            WebGlUniformKind::Other => {}
        }
        if WEBGL_LOG_UNIFORM_UPLOADS.fetch_add(1, Ordering::Relaxed) < 16 {
            log_str("qjs-webgl: uniformMatrix4fv loc=");
            log_usize_dec(loc as usize);
            log_str(" transpose=");
            log_usize_dec((transpose_f != 0.0) as usize);
            log_str("\n");
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_bind_buffer(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut target_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]);
        let target = target_f as i32;
        let mut buf_id_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut buf_id_f as *mut f64, args[1]) != 0 {
            return qjs::JSValue::undefined();
        }
        let buf_id = buf_id_f as i32;
        let mut st = WEBGL_STATE.lock();
        let buf_id = if buf_id > 0 { buf_id as u32 } else { 0 };
        match target as u32 {
            0x8892 => st.array_buffer = buf_id,         // ARRAY_BUFFER
            0x8893 => st.element_array_buffer = buf_id, // ELEMENT_ARRAY_BUFFER
            _ => {}
        }
        qjs::JSValue::undefined()
    }

    unsafe fn js_get_arraybuffer_view(
        ctx: *mut qjs::JSContext,
        val: qjs::JSValueConst,
    ) -> Option<(*const u8, usize)> {
        // Try TypedArray first.
        let mut byte_off: usize = 0;
        let mut byte_len: usize = 0;
        let mut bpe: usize = 0;
        let ab = qjs::JS_GetTypedArrayBuffer(
            ctx,
            val,
            &mut byte_off as *mut usize,
            &mut byte_len as *mut usize,
            &mut bpe as *mut usize,
        );
        if !ab.is_exception() && ab.tag != qjs::JS_TAG_UNDEFINED {
            let mut total: usize = 0;
            let ptr = qjs::JS_GetArrayBuffer(ctx, &mut total as *mut usize, ab);
            qjs::js_free_value(ctx, ab);
            if !ptr.is_null() {
                let start = byte_off.min(total);
                let end = start.saturating_add(byte_len).min(total);
                return Some((unsafe { ptr.add(start) } as *const u8, end.saturating_sub(start)));
            }
        } else {
            qjs::js_free_value(ctx, ab);
        }

        // Then plain ArrayBuffer.
        let mut total: usize = 0;
        let ptr = qjs::JS_GetArrayBuffer(ctx, &mut total as *mut usize, val);
        if ptr.is_null() {
            return None;
        }
        Some((ptr as *const u8, total))
    }

    unsafe extern "C" fn gl_active_texture(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut tex_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut tex_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let tex_enum = (tex_f as i32).max(0) as u32;
        let mut st = WEBGL_STATE.lock();
        st.textures.active_texture(tex_enum);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_bind_texture(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut target_f: f64 = 0.0;
        let mut tex_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]) != 0
            || qjs::JS_ToFloat64(ctx, &mut tex_f as *mut f64, args[1]) != 0
        {
            return qjs::JSValue::undefined();
        }
        let target = (target_f as i32).max(0) as u32;
        if target == 0x0DE1 {
            let tex_id = (tex_f as i32).max(0) as u32;
            let mut st = WEBGL_STATE.lock();
            st.textures.bind_texture_2d(tex_id);
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_pixel_store_i(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut pname_f: f64 = 0.0;
        let mut param_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut pname_f as *mut f64, args[0]) != 0
            || qjs::JS_ToFloat64(ctx, &mut param_f as *mut f64, args[1]) != 0
        {
            return qjs::JSValue::undefined();
        }
        let pname = (pname_f as i32).max(0) as u32;
        let param = param_f as i32;
        let mut st = WEBGL_STATE.lock();
        st.textures.pixel_store_i(pname, param);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_tex_parameter_i(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut target_f: f64 = 0.0;
        let mut pname_f: f64 = 0.0;
        let mut param_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]) != 0
            || qjs::JS_ToFloat64(ctx, &mut pname_f as *mut f64, args[1]) != 0
            || qjs::JS_ToFloat64(ctx, &mut param_f as *mut f64, args[2]) != 0
        {
            return qjs::JSValue::undefined();
        }
        let target = (target_f as i32).max(0) as u32;
        if target == 0x0DE1 {
            let pname = (pname_f as i32).max(0) as u32;
            let param = param_f as i32;
            let mut st = WEBGL_STATE.lock();
            st.textures.tex_parameter_i(pname, param);
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_tex_image_2d(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 9 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut target_f: f64 = 0.0;
        let mut level_f: f64 = 0.0;
        let mut internal_f: f64 = 0.0;
        let mut width_f: f64 = 0.0;
        let mut height_f: f64 = 0.0;
        let mut border_f: f64 = 0.0;
        let mut format_f: f64 = 0.0;
        let mut ty_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut level_f as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut internal_f as *mut f64, args[2]);
        let _ = qjs::JS_ToFloat64(ctx, &mut width_f as *mut f64, args[3]);
        let _ = qjs::JS_ToFloat64(ctx, &mut height_f as *mut f64, args[4]);
        let _ = qjs::JS_ToFloat64(ctx, &mut border_f as *mut f64, args[5]);
        let _ = qjs::JS_ToFloat64(ctx, &mut format_f as *mut f64, args[6]);
        let _ = qjs::JS_ToFloat64(ctx, &mut ty_f as *mut f64, args[7]);

        let target = (target_f as i32).max(0) as u32;
        if target != 0x0DE1 {
            return qjs::JSValue::undefined();
        }
        let level = level_f as i32;
        let width = width_f as i32;
        let height = height_f as i32;
        let border = border_f as i32;
        let format = (format_f as i32).max(0) as u32;
        let ty = (ty_f as i32).max(0) as u32;

        let data_opt = js_get_arraybuffer_view(ctx, args[8]).map(|(p, n)| core::slice::from_raw_parts(p, n));
        let mut st = WEBGL_STATE.lock();
        let _ = st
            .textures
            .tex_image_2d(level, width, height, border, format, ty, data_opt);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_tex_sub_image_2d(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 9 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut target_f: f64 = 0.0;
        let mut level_f: f64 = 0.0;
        let mut x_f: f64 = 0.0;
        let mut y_f: f64 = 0.0;
        let mut width_f: f64 = 0.0;
        let mut height_f: f64 = 0.0;
        let mut format_f: f64 = 0.0;
        let mut ty_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut level_f as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut x_f as *mut f64, args[2]);
        let _ = qjs::JS_ToFloat64(ctx, &mut y_f as *mut f64, args[3]);
        let _ = qjs::JS_ToFloat64(ctx, &mut width_f as *mut f64, args[4]);
        let _ = qjs::JS_ToFloat64(ctx, &mut height_f as *mut f64, args[5]);
        let _ = qjs::JS_ToFloat64(ctx, &mut format_f as *mut f64, args[6]);
        let _ = qjs::JS_ToFloat64(ctx, &mut ty_f as *mut f64, args[7]);
        let target = (target_f as i32).max(0) as u32;
        if target != 0x0DE1 {
            return qjs::JSValue::undefined();
        }

        let Some((p, n)) = js_get_arraybuffer_view(ctx, args[8]) else {
            return qjs::JSValue::undefined();
        };
        let src = core::slice::from_raw_parts(p, n);

        let level = level_f as i32;
        let xoffset = x_f as i32;
        let yoffset = y_f as i32;
        let width = width_f as i32;
        let height = height_f as i32;
        let format = (format_f as i32).max(0) as u32;
        let ty = (ty_f as i32).max(0) as u32;
        let mut st = WEBGL_STATE.lock();
        let _ = st
            .textures
            .tex_sub_image_2d(level, xoffset, yoffset, width, height, format, ty, src);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_buffer_data(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut target_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]);
        let target = target_f as i32;

        // WebGL allows bufferData(target, size, usage) as well as bufferData(target, data, usage).
        // Prefer buffer-like inputs first; numeric coercion of objects can produce NaN/0 and
        // accidentally drop real vertex/index payloads.
        let data_opt = js_get_arraybuffer_view(ctx, args[1]);
        let mut numeric_size: f64 = 0.0;
        if data_opt.is_none() {
            let _ = qjs::JS_ToFloat64(ctx, &mut numeric_size as *mut f64, args[1]);
        }

        let mut st = WEBGL_STATE.lock();
        let buf_id = match target as u32 {
            0x8892 => st.array_buffer,         // ARRAY_BUFFER
            0x8893 => st.element_array_buffer, // ELEMENT_ARRAY_BUFFER
            _ => 0,
        };
        if buf_id == 0 {
            return qjs::JSValue::undefined();
        }
        if let Some((ptr, len)) = data_opt {
            let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
            st.buffers.insert(buf_id, bytes.to_vec());
        } else {
            let sz = (numeric_size as i64).max(0) as usize;
            st.buffers.insert(buf_id, vec![0u8; sz]);
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_buffer_sub_data(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);

        let mut target_f: f64 = 0.0;
        let mut offset_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut offset_f as *mut f64, args[1]);
        let target = target_f as i32;
        let offset = (offset_f as i64).max(0) as usize;

        let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[2]) else {
            return qjs::JSValue::undefined();
        };
        let src = unsafe { core::slice::from_raw_parts(ptr, len) };

        let mut st = WEBGL_STATE.lock();
        let buf_id = match target as u32 {
            0x8892 => st.array_buffer,         // ARRAY_BUFFER
            0x8893 => st.element_array_buffer, // ELEMENT_ARRAY_BUFFER
            _ => 0,
        };
        if buf_id == 0 {
            return qjs::JSValue::undefined();
        }
        let dst = st.buffers.entry(buf_id).or_insert_with(Vec::new);
        let needed = offset.saturating_add(src.len());
        if needed > dst.len() {
            dst.resize(needed, 0);
        }
        dst[offset..offset + src.len()].copy_from_slice(src);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_enable_vertex_attrib_array(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut idx_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut idx_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let idx = (idx_f as i32).max(0) as u32;
        let mut st = WEBGL_STATE.lock();
        let entry = st.attribs.entry(idx).or_default();
        entry.enabled = true;
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_disable_vertex_attrib_array(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut idx_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut idx_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let idx = (idx_f as i32).max(0) as u32;
        let mut st = WEBGL_STATE.lock();
        let entry = st.attribs.entry(idx).or_default();
        entry.enabled = false;
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_create_vertex_array_oes(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let id = WEBGL_NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut st = WEBGL_STATE.lock();
        st.vaos.entry(id).or_insert_with(WebGlVaoState::default);
        js_int32(id as i32)
    }

    unsafe extern "C" fn gl_bind_vertex_array_oes(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let arg0 = args[0];
        let mut id_f: f64 = 0.0;
        let id = if arg0.tag == qjs::JS_TAG_NULL || arg0.tag == qjs::JS_TAG_UNDEFINED {
            0
        } else if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, arg0) == 0 {
            (id_f as i32).max(0) as u32
        } else {
            0
        };

        let mut st = WEBGL_STATE.lock();
        if id != 0 {
            st.vaos.entry(id).or_insert_with(WebGlVaoState::default);
        }
        webgl_save_current_vao_state(&mut st);
        webgl_load_vao_state(&mut st, id);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_delete_vertex_array_oes(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut id_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let id = (id_f as i32).max(0) as u32;
        if id == 0 {
            return qjs::JSValue::undefined();
        }
        let mut st = WEBGL_STATE.lock();
        if st.current_vao == id {
            webgl_load_vao_state(&mut st, 0);
        }
        st.vaos.remove(&id);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_is_vertex_array_oes(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_bool(false);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut id_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0 {
            return js_bool(false);
        }
        let id = (id_f as i32).max(0) as u32;
        if id == 0 {
            return js_bool(false);
        }
        let st = WEBGL_STATE.lock();
        js_bool(st.vaos.contains_key(&id))
    }

    unsafe extern "C" fn gl_vertex_attrib_pointer(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 6 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);

        let mut idx_f: f64 = 0.0;
        let mut size_f: f64 = 0.0;
        let mut ty_f: f64 = 0.0;
        let mut stride_f: f64 = 0.0;
        let mut offset_f: f64 = 0.0;
        let mut normalized_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut idx_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut size_f as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut ty_f as *mut f64, args[2]);
        let _ = qjs::JS_ToFloat64(ctx, &mut normalized_f as *mut f64, args[3]);
        let normalized = normalized_f != 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut stride_f as *mut f64, args[4]);
        let _ = qjs::JS_ToFloat64(ctx, &mut offset_f as *mut f64, args[5]);

        let idx = (idx_f as i32).max(0) as u32;
        let size = (size_f as i32).max(0);
        let ty = (ty_f as i32).max(0) as u32;
        let stride = (stride_f as i32).max(0);
        let offset = (offset_f as i64).max(0) as usize;

        let mut st = WEBGL_STATE.lock();
        let array_buffer = st.array_buffer;
        let entry = st.attribs.entry(idx).or_default();
        entry.size = size;
        entry.ty = ty;
        entry.normalized = normalized;
        entry.stride = stride;
        entry.offset = offset;
        entry.buffer = array_buffer;
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_viewport(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 4 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut w_f: f64 = 0.0;
        let mut h_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut w_f as *mut f64, args[2]);
        let _ = qjs::JS_ToFloat64(ctx, &mut h_f as *mut f64, args[3]);
        let w = (w_f as i32).max(0);
        let h = (h_f as i32).max(0);
        let mut st = WEBGL_STATE.lock();
        st.viewport_w = w;
        st.viewport_h = h;
        qjs::JSValue::undefined()
    }

    fn gl_cap_get(st: &WebGlState, cap: u32) -> Option<bool> {
        match cap {
            0x0BE2 => Some(st.enabled_blend),        // BLEND
            0x0B44 => Some(st.enabled_cull_face),    // CULL_FACE
            0x0B71 => Some(st.enabled_depth_test),   // DEPTH_TEST
            0x0C11 => Some(st.enabled_scissor_test), // SCISSOR_TEST
            _ => None,
        }
    }

    fn gl_cap_set(st: &mut WebGlState, cap: u32, enabled: bool) {
        match cap {
            0x0BE2 => st.enabled_blend = enabled,        // BLEND
            0x0B44 => st.enabled_cull_face = enabled,    // CULL_FACE
            0x0B71 => st.enabled_depth_test = enabled,   // DEPTH_TEST
            0x0C11 => st.enabled_scissor_test = enabled, // SCISSOR_TEST
            _ => {}
        }
    }

    unsafe extern "C" fn gl_enable(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut cap_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut cap_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let cap = (cap_f as i32).max(0) as u32;
        gl_cap_set(&mut WEBGL_STATE.lock(), cap, true);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_disable(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut cap_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut cap_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let cap = (cap_f as i32).max(0) as u32;
        gl_cap_set(&mut WEBGL_STATE.lock(), cap, false);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_is_enabled(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_bool(false);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut cap_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut cap_f as *mut f64, args[0]) != 0 {
            return js_bool(false);
        }
        let cap = (cap_f as i32).max(0) as u32;
        let st = WEBGL_STATE.lock();
        js_bool(gl_cap_get(&st, cap).unwrap_or(false))
    }

    unsafe extern "C" fn gl_front_face(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut mode_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut mode_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let mode = (mode_f as i32).max(0) as u32;
        if mode == 0x0900 || mode == 0x0901 {
            WEBGL_STATE.lock().front_face_mode = mode;
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_cull_face(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut mode_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut mode_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let mode = (mode_f as i32).max(0) as u32;
        if mode == 0x0404 || mode == 0x0405 || mode == 0x0408 {
            WEBGL_STATE.lock().cull_face_mode = mode;
        }
        qjs::JSValue::undefined()
    }

    fn ty_size_bytes(ty: u32) -> Option<usize> {
        match ty {
            0x1400 => Some(1), // BYTE
            0x1406 => Some(4), // FLOAT
            0x1401 => Some(1), // UNSIGNED_BYTE
            0x1402 => Some(2), // SHORT
            0x1403 => Some(2), // UNSIGNED_SHORT
            0x1405 => Some(4), // UNSIGNED_INT
            _ => None,
        }
    }

    fn read_u16_le(bytes: &[u8], off: usize) -> Option<u16> {
        let b0 = *bytes.get(off)?;
        let b1 = *bytes.get(off + 1)?;
        Some(u16::from_le_bytes([b0, b1]))
    }

    fn read_f32_le(bytes: &[u8], off: usize) -> Option<f32> {
        let b0 = *bytes.get(off)?;
        let b1 = *bytes.get(off + 1)?;
        let b2 = *bytes.get(off + 2)?;
        let b3 = *bytes.get(off + 3)?;
        Some(f32::from_le_bytes([b0, b1, b2, b3]))
    }

    fn read_i16_le(bytes: &[u8], off: usize) -> Option<i16> {
        let b0 = *bytes.get(off)?;
        let b1 = *bytes.get(off + 1)?;
        Some(i16::from_le_bytes([b0, b1]))
    }

    fn read_u32_le(bytes: &[u8], off: usize) -> Option<u32> {
        let b0 = *bytes.get(off)?;
        let b1 = *bytes.get(off + 1)?;
        let b2 = *bytes.get(off + 2)?;
        let b3 = *bytes.get(off + 3)?;
        Some(u32::from_le_bytes([b0, b1, b2, b3]))
    }

    fn read_attrib_component(bytes: &[u8], off: usize, ty: u32, normalized: bool) -> Option<f32> {
        match ty {
            0x1406 => read_f32_le(bytes, off), // FLOAT
            0x1401 => {
                let v = *bytes.get(off)? as f32; // UNSIGNED_BYTE
                Some(if normalized { v / 255.0 } else { v })
            }
            0x1400 => {
                let v = *bytes.get(off)? as i8 as f32; // BYTE
                Some(if normalized { (v / 127.0).max(-1.0) } else { v })
            }
            0x1403 => {
                let v = read_u16_le(bytes, off)? as f32; // UNSIGNED_SHORT
                Some(if normalized { v / 65535.0 } else { v })
            }
            0x1402 => {
                let v = read_i16_le(bytes, off)? as f32; // SHORT
                Some(if normalized { (v / 32767.0).max(-1.0) } else { v })
            }
            0x1405 => Some(read_u32_le(bytes, off)? as f32), // UNSIGNED_INT
            _ => None,
        }
    }

    fn clamp01(v: f32) -> f32 {
        if v < 0.0 {
            0.0
        } else if v > 1.0 {
            1.0
        } else {
            v
        }
    }

    fn blend_factor_rgb(
        factor: u32,
        src: [f32; 3],
        src_a: f32,
        dst: [f32; 3],
        dst_a: f32,
    ) -> [f32; 3] {
        match factor {
            0 => [0.0, 0.0, 0.0], // ZERO
            1 => [1.0, 1.0, 1.0], // ONE
            0x0302 => [src_a, src_a, src_a], // SRC_ALPHA
            0x0303 => [1.0 - src_a, 1.0 - src_a, 1.0 - src_a], // ONE_MINUS_SRC_ALPHA
            0x0304 => [dst_a, dst_a, dst_a], // DST_ALPHA
            0x0305 => [1.0 - dst_a, 1.0 - dst_a, 1.0 - dst_a], // ONE_MINUS_DST_ALPHA
            0x0300 => src, // SRC_COLOR
            0x0301 => [1.0 - src[0], 1.0 - src[1], 1.0 - src[2]], // ONE_MINUS_SRC_COLOR
            0x0306 => dst, // DST_COLOR
            0x0307 => [1.0 - dst[0], 1.0 - dst[1], 1.0 - dst[2]], // ONE_MINUS_DST_COLOR
            _ => [1.0, 1.0, 1.0],
        }
    }

    fn apply_blend_equation(eq: u32, s: f32, d: f32) -> f32 {
        match eq {
            0x800A => s - d, // FUNC_SUBTRACT
            0x800B => d - s, // FUNC_REVERSE_SUBTRACT
            _ => s + d,      // FUNC_ADD
        }
    }

    fn apply_simple_blend_rgb(
        enabled: bool,
        src_rgb_u8: [u8; 3],
        src_a_u8: u8,
        clear_rgb: u32,
        src_factor: u32,
        dst_factor: u32,
        eq: u32,
    ) -> [u8; 3] {
        if !enabled {
            return src_rgb_u8;
        }
        let src = [
            (src_rgb_u8[0] as f32) / 255.0,
            (src_rgb_u8[1] as f32) / 255.0,
            (src_rgb_u8[2] as f32) / 255.0,
        ];
        let src_a = (src_a_u8 as f32) / 255.0;
        let dst = [
            ((clear_rgb >> 16) as u8 as f32) / 255.0,
            (((clear_rgb >> 8) & 0xFF) as u8 as f32) / 255.0,
            ((clear_rgb & 0xFF) as u8 as f32) / 255.0,
        ];
        let dst_a = 1.0;
        let sf = blend_factor_rgb(src_factor, src, src_a, dst, dst_a);
        let df = blend_factor_rgb(dst_factor, src, src_a, dst, dst_a);
        let mut out = [0u8; 3];
        for i in 0..3 {
            let s = src[i] * sf[i];
            let d = dst[i] * df[i];
            let v = clamp01(apply_blend_equation(eq, s, d));
            out[i] = (v * 255.0 + 0.5) as u8;
        }
        out
    }

    fn is_valid_blend_factor(v: u32) -> bool {
        matches!(
            v,
            0 | 1 | 0x0302 | 0x0303 | 0x0304 | 0x0305 | 0x0300 | 0x0301 | 0x0306 | 0x0307
        )
    }

    fn is_valid_blend_equation(v: u32) -> bool {
        matches!(v, 0x8006 | 0x800A | 0x800B)
    }

    unsafe extern "C" fn gl_blend_func(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut src_f: f64 = 0.0;
        let mut dst_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut src_f as *mut f64, args[0]) != 0
            || qjs::JS_ToFloat64(ctx, &mut dst_f as *mut f64, args[1]) != 0
        {
            return qjs::JSValue::undefined();
        }
        let src = (src_f as i32).max(0) as u32;
        let dst = (dst_f as i32).max(0) as u32;
        if is_valid_blend_factor(src) && is_valid_blend_factor(dst) {
            let mut st = WEBGL_STATE.lock();
            st.blend_src_rgb = src;
            st.blend_dst_rgb = dst;
            st.blend_src_alpha = src;
            st.blend_dst_alpha = dst;
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_blend_func_separate(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 4 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut sr: f64 = 0.0;
        let mut dr: f64 = 0.0;
        let mut sa: f64 = 0.0;
        let mut da: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut sr as *mut f64, args[0]) != 0
            || qjs::JS_ToFloat64(ctx, &mut dr as *mut f64, args[1]) != 0
            || qjs::JS_ToFloat64(ctx, &mut sa as *mut f64, args[2]) != 0
            || qjs::JS_ToFloat64(ctx, &mut da as *mut f64, args[3]) != 0
        {
            return qjs::JSValue::undefined();
        }
        let sr = (sr as i32).max(0) as u32;
        let dr = (dr as i32).max(0) as u32;
        let sa = (sa as i32).max(0) as u32;
        let da = (da as i32).max(0) as u32;
        if is_valid_blend_factor(sr)
            && is_valid_blend_factor(dr)
            && is_valid_blend_factor(sa)
            && is_valid_blend_factor(da)
        {
            let mut st = WEBGL_STATE.lock();
            st.blend_src_rgb = sr;
            st.blend_dst_rgb = dr;
            st.blend_src_alpha = sa;
            st.blend_dst_alpha = da;
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_blend_equation(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut mode_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut mode_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let mode = (mode_f as i32).max(0) as u32;
        if is_valid_blend_equation(mode) {
            let mut st = WEBGL_STATE.lock();
            st.blend_eq_rgb = mode;
            st.blend_eq_alpha = mode;
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_blend_equation_separate(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut rgb_f: f64 = 0.0;
        let mut alpha_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut rgb_f as *mut f64, args[0]) != 0
            || qjs::JS_ToFloat64(ctx, &mut alpha_f as *mut f64, args[1]) != 0
        {
            return qjs::JSValue::undefined();
        }
        let rgb = (rgb_f as i32).max(0) as u32;
        let alpha = (alpha_f as i32).max(0) as u32;
        if is_valid_blend_equation(rgb) && is_valid_blend_equation(alpha) {
            let mut st = WEBGL_STATE.lock();
            st.blend_eq_rgb = rgb;
            st.blend_eq_alpha = alpha;
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_draw_elements(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 4 {
            webgl_log_draw_drop("drawElements", "bad-args");
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut mode_f: f64 = 0.0;
        let mut count_f: f64 = 0.0;
        let mut ty_f: f64 = 0.0;
        let mut offset_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut mode_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut count_f as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut ty_f as *mut f64, args[2]);
        let _ = qjs::JS_ToFloat64(ctx, &mut offset_f as *mut f64, args[3]);
        let mode = mode_f as i32;
        if mode != 0x0004 {
            webgl_log_draw_drop("drawElements", "mode!=TRIANGLES");
            return qjs::JSValue::undefined();
        }
        let mut count = (count_f as i32).max(0) as usize;
        count -= count % 3;
        if count == 0 {
            webgl_log_draw_drop("drawElements", "count==0");
            return qjs::JSValue::undefined();
        }
        let ty = (ty_f as i32).max(0) as u32;
        if ty != 0x1403 {
            // UNSIGNED_SHORT only for now
            webgl_log_draw_drop("drawElements", "index-type!=UNSIGNED_SHORT");
            return qjs::JSValue::undefined();
        }
        let index_off = (offset_f as i64).max(0) as usize;

        const VTX_SIZE: usize = 12;

        // Snapshot everything we need while holding the lock.
        let (
            clear_rgb,
            viewport_w,
            viewport_h,
            buffers,
            element_array_buffer,
            elem_bytes,
            attribs,
            enabled_blend,
            blend_src_rgb,
            blend_dst_rgb,
            blend_eq_rgb,
            has_translation_matrix,
            has_projection_matrix,
            translation_matrix,
            projection_matrix,
        ) = {
            let st = WEBGL_STATE.lock();
            let Some(elem_bytes) = st.buffers.get(&st.element_array_buffer) else {
                webgl_log_draw_drop("drawElements", "no-element-array-buffer");
                return qjs::JSValue::undefined();
            };
            (
                st.clear_rgb,
                st.viewport_w,
                st.viewport_h,
                st.buffers.clone(),
                st.element_array_buffer,
                elem_bytes.clone(),
                st.attribs.clone(),
                st.enabled_blend,
                st.blend_src_rgb,
                st.blend_dst_rgb,
                st.blend_eq_rgb,
                st.has_translation_matrix,
                st.has_projection_matrix,
                st.translation_matrix,
                st.projection_matrix,
            )
        };

        let viewport_w = (viewport_w.max(1)) as f32;
        let viewport_h = (viewport_h.max(1)) as f32;

        // Heuristic: pick likely position attribute.
        let mut pos_attr: Option<(u32, WebGlVertexAttrib)> = None;
        let mut col_attr: Option<(u32, WebGlVertexAttrib)> = None;
        for (idx, a) in attribs.iter() {
            if !a.enabled {
                continue;
            }
            if pos_attr.is_none() && a.size >= 2 && a.ty == 0x1406 {
                pos_attr = Some((*idx, *a));
            }
            if col_attr.is_none() && a.size == 4 && a.ty == 0x1401 {
                col_attr = Some((*idx, *a));
            }
        }
        if pos_attr.is_none() {
            for (idx, a) in attribs.iter() {
                if a.enabled && a.size >= 2 && ty_size_bytes(a.ty).is_some() {
                    pos_attr = Some((*idx, *a));
                    break;
                }
            }
        }
        if pos_attr.is_none() {
            for (idx, a) in attribs.iter() {
                if a.size >= 2 && a.ty == 0x1406 {
                    pos_attr = Some((*idx, *a));
                    break;
                }
            }
        }
        if pos_attr.is_none() {
            for (idx, a) in attribs.iter() {
                if a.size >= 2 && ty_size_bytes(a.ty).is_some() {
                    pos_attr = Some((*idx, *a));
                    break;
                }
            }
        }
        if pos_attr.is_none() {
            for (buf_id, bytes) in buffers.iter() {
                if *buf_id == element_array_buffer || bytes.len() < 8 {
                    continue;
                }
                pos_attr = Some((
                    0,
                    WebGlVertexAttrib {
                        enabled: true,
                        size: 2,
                        ty: 0x1406,
                        normalized: false,
                        stride: 0,
                        offset: 0,
                        buffer: *buf_id,
                    },
                ));
                break;
            }
        }
        let Some((_pos_idx, pos)) = pos_attr else {
            webgl_log_draw_drop("drawElements", "no-pos-attrib");
            return qjs::JSValue::undefined();
        };

        let pos_ty_sz = match ty_size_bytes(pos.ty) {
            Some(v) => v,
            None => return qjs::JSValue::undefined(),
        };
        let pos_stride = if pos.stride == 0 {
            (pos.size as usize).saturating_mul(pos_ty_sz)
        } else {
            pos.stride as usize
        };
        if pos_stride == 0 {
            webgl_log_draw_drop("drawElements", "pos-stride==0");
            return qjs::JSValue::undefined();
        }

        let (col, col_stride) = if let Some((_col_idx, col)) = col_attr {
            let col_ty_sz = match ty_size_bytes(col.ty) {
                Some(v) => v,
                None => 0,
            };
            let stride = if col.stride == 0 {
                (col.size as usize).saturating_mul(col_ty_sz)
            } else {
                col.stride as usize
            };
            (Some(col), stride)
        } else {
            (None, 0)
        };

        let Some(pos_bytes) = buffers.get(&pos.buffer) else {
            webgl_log_draw_drop("drawElements", "pos-buffer-missing");
            return qjs::JSValue::undefined();
        };
        let col_bytes_opt = col.and_then(|c| buffers.get(&c.buffer));

        let mut out = Vec::with_capacity(count.saturating_mul(VTX_SIZE));
        for i in 0..count {
            let idx_off = index_off.saturating_add(i.saturating_mul(2));
            // If index data is missing/truncated, degrade to sequential indexing.
            let vtx_idx = read_u16_le(&elem_bytes, idx_off)
                .map(|v| v as usize)
                .unwrap_or(i);

            let base = vtx_idx.saturating_mul(pos_stride).saturating_add(pos.offset);
            let Some(x_px) = read_attrib_component(pos_bytes, base, pos.ty, pos.normalized) else {
                continue;
            };
            let Some(y_px) = read_attrib_component(
                pos_bytes,
                base.saturating_add(pos_ty_sz),
                pos.ty,
                pos.normalized,
            ) else {
                continue;
            };

            // If Pixi fed us the transform uniforms, emulate:
            //   gl_Position = projectionMatrix * translationMatrix * vec3(aVertexPosition, 1.0)
            // Otherwise keep legacy viewport mapping so older content still works.
            let (x, y) = if has_translation_matrix && has_projection_matrix {
                let (tx, ty, tz) = mat3_mul_vec3(&translation_matrix, x_px, y_px, 1.0);
                let (cx, cy, _cz) = mat3_mul_vec3(&projection_matrix, tx, ty, tz);
                (cx, cy)
            } else {
                let x = (2.0 * (x_px / viewport_w)) - 1.0;
                let y = 1.0 - (2.0 * (y_px / viewport_h));
                (x, y)
            };

            let (r, g, b) = if let (Some(col), Some(col_bytes)) = (col, col_bytes_opt) {
                let base = vtx_idx
                    .saturating_mul(col_stride)
                    .saturating_add(col.offset);
                let r = *col_bytes.get(base).unwrap_or(&255);
                let g = *col_bytes.get(base + 1).unwrap_or(&255);
                let b = *col_bytes.get(base + 2).unwrap_or(&255);
                let a = *col_bytes.get(base + 3).unwrap_or(&255);
                let rgb = apply_simple_blend_rgb(
                    enabled_blend,
                    [r, g, b],
                    a,
                    clear_rgb,
                    blend_src_rgb,
                    blend_dst_rgb,
                    blend_eq_rgb,
                );
                (rgb[0], rgb[1], rgb[2])
            } else {
                (255, 255, 255)
            };

            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
            out.push(r);
            out.push(g);
            out.push(b);
            out.push(0);
        }

        if out.is_empty() {
            // Fallback: try common interleaved float2 position layouts from any ARRAY_BUFFER.
            // This keeps Pixi advancing when attribute/index state is partially modeled.
            let mut recovered = Vec::new();
            let try_strides = [8usize, 12, 16, 20, 24, 28, 32];
            for (buf_id, bytes) in buffers.iter() {
                if *buf_id == element_array_buffer || bytes.len() < 8 {
                    continue;
                }
                for stride in try_strides.iter() {
                    let mut tmp = Vec::with_capacity(count.saturating_mul(VTX_SIZE));
                    for i in 0..count {
                        let base = i.saturating_mul(*stride);
                        let Some(x_px) = read_f32_le(bytes, base) else {
                            break;
                        };
                        let Some(y_px) = read_f32_le(bytes, base.saturating_add(4)) else {
                            break;
                        };
                        let (x, y) = if has_translation_matrix && has_projection_matrix {
                            let (tx, ty, tz) = mat3_mul_vec3(&translation_matrix, x_px, y_px, 1.0);
                            let (cx, cy, _cz) = mat3_mul_vec3(&projection_matrix, tx, ty, tz);
                            (cx, cy)
                        } else {
                            let x = (2.0 * (x_px / viewport_w)) - 1.0;
                            let y = 1.0 - (2.0 * (y_px / viewport_h));
                            (x, y)
                        };
                        tmp.extend_from_slice(&x.to_le_bytes());
                        tmp.extend_from_slice(&y.to_le_bytes());
                        tmp.push(255);
                        tmp.push(255);
                        tmp.push(255);
                        tmp.push(0);
                    }
                    if !tmp.is_empty() {
                        recovered = tmp;
                        break;
                    }
                }
                if !recovered.is_empty() {
                    break;
                }
            }
            if !recovered.is_empty() {
                out = recovered;
            }
        }

        if out.is_empty() {
            webgl_log_draw_drop("drawElements", "out-empty");
            return qjs::JSValue::undefined();
        }

        if WEBGL_LOG_DRAW_MODE.fetch_add(1, Ordering::Relaxed) < 12 {
            log_str("qjs-webgl: drawElements matrix_path=");
            log_usize_dec((has_translation_matrix && has_projection_matrix) as usize);
            log_str(" count=");
            log_usize_dec(count);
            log_str("\n");
        }

        if WEBGL_DID_LOG_DRAW
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            log_str("qjs-webgl: drawElements -> gfx vtx_bytes=");
            log_usize_dec(out.len());
            log_str("\n");
        }
        unsafe {
            let _ = trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, out.as_ptr(), out.len());
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_get_supported_extensions(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // Return an empty list rather than `null`.
        qjs::JS_NewArray(ctx)
    }

    unsafe extern "C" fn gl_get_shader_precision_format(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // Best-effort, plausible defaults.
        let obj = qjs::JS_NewObject(ctx);
        if obj.is_exception() {
            return obj;
        }
        let k_range_min = b"rangeMin\0";
        let k_range_max = b"rangeMax\0";
        let k_prec = b"precision\0";
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            k_range_min.as_ptr() as *const c_char,
            js_int32(127),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            k_range_max.as_ptr() as *const c_char,
            js_int32(127),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            k_prec.as_ptr() as *const c_char,
            js_int32(23),
        );
        obj
    }

    unsafe extern "C" fn gl_clear_color(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut rf: f64 = 0.0;
        let mut gf: f64 = 0.0;
        let mut bf: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut rf as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut gf as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut bf as *mut f64, args[2]);
        let clamp = |v: f64| -> u8 {
            let x = if v.is_nan() { 0.0 } else { v };
            let x = x.max(0.0).min(1.0);
            (x * 255.0 + 0.5) as u8
        };
        let r = clamp(rf);
        let g = clamp(gf);
        let b = clamp(bf);
        let rgb = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        WEBGL_STATE.lock().clear_rgb = rgb;
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_clear(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut mask_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut mask_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let mask = (mask_f as i32).max(0) as u32;
        if (mask & 0x4000) != 0 {
            let clear_rgb = WEBGL_STATE.lock().clear_rgb;
            unsafe {
                let _ = trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, core::ptr::null(), 0);
            }
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_draw_arrays(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            webgl_log_draw_drop("drawArrays", "bad-args");
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut mode_f: f64 = 0.0;
        let mut first_f: f64 = 0.0;
        let mut count_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut mode_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut first_f as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut count_f as *mut f64, args[2]);
        let mode = mode_f as i32;
        // TRIANGLES only.
        if mode != 0x0004 {
            webgl_log_draw_drop("drawArrays", "mode!=TRIANGLES");
            return qjs::JSValue::undefined();
        }
        let first = (first_f as i32).max(0) as usize;
        let mut count = (count_f as i32).max(0) as usize;
        count -= count % 3;
        if count == 0 {
            webgl_log_draw_drop("drawArrays", "count==0");
            return qjs::JSValue::undefined();
        }

        const VTX_SIZE: usize = 12;

        // Snapshot everything we need while holding the lock.
        let (
            clear_rgb,
            viewport_w,
            viewport_h,
            buffers,
            attribs,
            enabled_blend,
            blend_src_rgb,
            blend_dst_rgb,
            blend_eq_rgb,
            has_translation_matrix,
            has_projection_matrix,
            translation_matrix,
            projection_matrix,
        ) = {
            let st = WEBGL_STATE.lock();
            (
                st.clear_rgb,
                st.viewport_w,
                st.viewport_h,
                st.buffers.clone(),
                st.attribs.clone(),
                st.enabled_blend,
                st.blend_src_rgb,
                st.blend_dst_rgb,
                st.blend_eq_rgb,
                st.has_translation_matrix,
                st.has_projection_matrix,
                st.translation_matrix,
                st.projection_matrix,
            )
        };

        let viewport_w = (viewport_w.max(1)) as f32;
        let viewport_h = (viewport_h.max(1)) as f32;

        // Heuristic: pick likely position attribute.
        let mut pos_attr: Option<(u32, WebGlVertexAttrib)> = None;
        let mut col_attr: Option<(u32, WebGlVertexAttrib)> = None;
        for (idx, a) in attribs.iter() {
            if !a.enabled {
                continue;
            }
            if pos_attr.is_none() && a.size >= 2 && a.ty == 0x1406 {
                pos_attr = Some((*idx, *a));
            }
            if col_attr.is_none() && a.size == 4 && a.ty == 0x1401 {
                col_attr = Some((*idx, *a));
            }
        }
        if pos_attr.is_none() {
            for (idx, a) in attribs.iter() {
                if a.enabled && a.size >= 2 && ty_size_bytes(a.ty).is_some() {
                    pos_attr = Some((*idx, *a));
                    break;
                }
            }
        }
        if pos_attr.is_none() {
            for (idx, a) in attribs.iter() {
                if a.size >= 2 && a.ty == 0x1406 {
                    pos_attr = Some((*idx, *a));
                    break;
                }
            }
        }
        if pos_attr.is_none() {
            for (idx, a) in attribs.iter() {
                if a.size >= 2 && ty_size_bytes(a.ty).is_some() {
                    pos_attr = Some((*idx, *a));
                    break;
                }
            }
        }
        if pos_attr.is_none() {
            for (buf_id, bytes) in buffers.iter() {
                if bytes.len() < 8 {
                    continue;
                }
                pos_attr = Some((
                    0,
                    WebGlVertexAttrib {
                        enabled: true,
                        size: 2,
                        ty: 0x1406,
                        normalized: false,
                        stride: 0,
                        offset: 0,
                        buffer: *buf_id,
                    },
                ));
                break;
            }
        }
        let Some((_pos_idx, pos)) = pos_attr else {
            webgl_log_draw_drop("drawArrays", "no-pos-attrib");
            return qjs::JSValue::undefined();
        };

        let pos_ty_sz = match ty_size_bytes(pos.ty) {
            Some(v) => v,
            None => {
                webgl_log_draw_drop("drawArrays", "bad-pos-type");
                return qjs::JSValue::undefined();
            }
        };
        let pos_stride = if pos.stride == 0 {
            (pos.size as usize).saturating_mul(pos_ty_sz)
        } else {
            pos.stride as usize
        };
        if pos_stride == 0 {
            webgl_log_draw_drop("drawArrays", "pos-stride==0");
            return qjs::JSValue::undefined();
        }

        let (col, col_stride) = if let Some((_col_idx, col)) = col_attr {
            let col_ty_sz = match ty_size_bytes(col.ty) {
                Some(v) => v,
                None => 0,
            };
            let stride = if col.stride == 0 {
                (col.size as usize).saturating_mul(col_ty_sz)
            } else {
                col.stride as usize
            };
            (Some(col), stride)
        } else {
            (None, 0)
        };

        let Some(pos_bytes) = buffers.get(&pos.buffer) else {
            webgl_log_draw_drop("drawArrays", "pos-buffer-missing");
            return qjs::JSValue::undefined();
        };
        let col_bytes_opt = col.and_then(|c| buffers.get(&c.buffer));

        let mut out = Vec::with_capacity(count.saturating_mul(VTX_SIZE));
        for i in 0..count {
            let vtx_idx = first.saturating_add(i);

            let base = vtx_idx.saturating_mul(pos_stride).saturating_add(pos.offset);
            let Some(x_px) = read_attrib_component(pos_bytes, base, pos.ty, pos.normalized) else {
                continue;
            };
            let Some(y_px) = read_attrib_component(
                pos_bytes,
                base.saturating_add(pos_ty_sz),
                pos.ty,
                pos.normalized,
            ) else {
                continue;
            };

            let (x, y) = if has_translation_matrix && has_projection_matrix {
                let (tx, ty, tz) = mat3_mul_vec3(&translation_matrix, x_px, y_px, 1.0);
                let (cx, cy, _cz) = mat3_mul_vec3(&projection_matrix, tx, ty, tz);
                (cx, cy)
            } else {
                let x = (2.0 * (x_px / viewport_w)) - 1.0;
                let y = 1.0 - (2.0 * (y_px / viewport_h));
                (x, y)
            };

            let (r, g, b) = if let (Some(col), Some(col_bytes)) = (col, col_bytes_opt) {
                let base = vtx_idx
                    .saturating_mul(col_stride)
                    .saturating_add(col.offset);
                let r = *col_bytes.get(base).unwrap_or(&255);
                let g = *col_bytes.get(base + 1).unwrap_or(&255);
                let b = *col_bytes.get(base + 2).unwrap_or(&255);
                let a = *col_bytes.get(base + 3).unwrap_or(&255);
                let rgb = apply_simple_blend_rgb(
                    enabled_blend,
                    [r, g, b],
                    a,
                    clear_rgb,
                    blend_src_rgb,
                    blend_dst_rgb,
                    blend_eq_rgb,
                );
                (rgb[0], rgb[1], rgb[2])
            } else {
                (255, 255, 255)
            };

            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
            out.push(r);
            out.push(g);
            out.push(b);
            out.push(0);
        }

        if out.is_empty() {
            webgl_log_draw_drop("drawArrays", "out-empty");
            return qjs::JSValue::undefined();
        }

        if WEBGL_LOG_DRAW_MODE.fetch_add(1, Ordering::Relaxed) < 12 {
            log_str("qjs-webgl: drawArrays matrix_path=");
            log_usize_dec((has_translation_matrix && has_projection_matrix) as usize);
            log_str(" count=");
            log_usize_dec(count);
            log_str("\n");
        }

        unsafe {
            if WEBGL_DID_LOG_DRAW
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                log_str("qjs-webgl: drawArrays -> gfx vtx_bytes=");
                log_usize_dec(out.len());
                log_str("\n");
            }
            let _ = trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, out.as_ptr(), out.len());
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_get_parameter(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // Return safe defaults for common queries.
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut pname_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut pname_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let pname = pname_f as i32;
        let st = WEBGL_STATE.lock();
        match pname as u32 {
            // MAX_TEXTURE_SIZE
            0x0D33 => js_int32(4096),
            // MAX_TEXTURE_IMAGE_UNITS
            0x8872 => js_int32(8),
            // MAX_VERTEX_ATTRIBS
            0x8869 => js_int32(8),
            // SAMPLES
            0x80A9 => js_int32(1),
            // FRAMEBUFFER_BINDING / DRAW_FRAMEBUFFER_BINDING
            0x8CA6 => js_null(),
            // CULL_FACE_MODE
            0x0B45 => js_int32(st.cull_face_mode as i32),
            // FRONT_FACE
            0x0B46 => js_int32(st.front_face_mode as i32),
            // BLEND_SRC_RGB
            0x80C9 => js_int32(st.blend_src_rgb as i32),
            // BLEND_DST_RGB
            0x80C8 => js_int32(st.blend_dst_rgb as i32),
            // BLEND_SRC_ALPHA
            0x80CB => js_int32(st.blend_src_alpha as i32),
            // BLEND_DST_ALPHA
            0x80CA => js_int32(st.blend_dst_alpha as i32),
            // BLEND_EQUATION / BLEND_EQUATION_RGB
            0x8009 => js_int32(st.blend_eq_rgb as i32),
            // BLEND_EQUATION_ALPHA
            0x883D => js_int32(st.blend_eq_alpha as i32),
            // BLEND / CULL_FACE / DEPTH_TEST / SCISSOR_TEST
            0x0BE2 | 0x0B44 | 0x0B71 | 0x0C11 => js_bool(gl_cap_get(&st, pname as u32).unwrap_or(false)),
            // VERSION
            0x1F02 => {
                let s = b"WebGL 1.0 (TRUEOS shim)\0";
                qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len() - 1)
            }
            // VENDOR
            0x1F00 => {
                let s = b"TRUEOS\0";
                qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len() - 1)
            }
            // RENDERER
            0x1F01 => {
                let s = b"software\0";
                qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len() - 1)
            }
            _ => js_null(),
        }
    }

    unsafe extern "C" fn gl_get_error(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // NO_ERROR
        js_int32(0)
    }

    unsafe extern "C" fn gl_get_program_parameter(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return js_bool(true);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut program_f: f64 = 0.0;
        let mut pname_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut program_f as *mut f64, args[0]);
        if qjs::JS_ToFloat64(ctx, &mut pname_f as *mut f64, args[1]) != 0 {
            return js_bool(true);
        }
        let program_id = (program_f as i32).max(0) as u32;
        let mut st = WEBGL_STATE.lock();
        if program_id != 0 && !st.program_active_attribs.contains_key(&program_id) {
            refresh_program_active_attribs(&mut st, program_id);
        }
        match (pname_f as i32).max(0) as u32 {
            // LINK_STATUS
            0x8B82 => js_bool(true),
            // ACTIVE_ATTRIBUTES / ACTIVE_UNIFORMS
            0x8B89 => js_int32(
                st.program_active_attribs
                    .get(&program_id)
                    .map(|v| v.len() as i32)
                    .unwrap_or(0),
            ),
            0x8B86 => js_int32(0),
            _ => js_bool(true),
        }
    }

    unsafe extern "C" fn gl_get_active_attrib(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return js_null();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut program_f: f64 = 0.0;
        let mut idx_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut program_f as *mut f64, args[0]);
        if qjs::JS_ToFloat64(ctx, &mut idx_f as *mut f64, args[1]) != 0 {
            return js_null();
        }
        let program_id = (program_f as i32).max(0) as u32;
        let idx = (idx_f as i32).max(0) as usize;
        let mut st = WEBGL_STATE.lock();
        if program_id != 0 && !st.program_active_attribs.contains_key(&program_id) {
            refresh_program_active_attribs(&mut st, program_id);
        }
        let Some(attr) = st
            .program_active_attribs
            .get(&program_id)
            .and_then(|v| v.get(idx))
            .cloned()
        else {
            return js_null();
        };
        let obj = qjs::JS_NewObject(ctx);
        if obj.is_exception() {
            return obj;
        }
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            b"name\0".as_ptr() as *const c_char,
            qjs::JS_NewStringLen(ctx, attr.name.as_ptr() as *const c_char, attr.name.len()),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            b"size\0".as_ptr() as *const c_char,
            js_int32(attr.size),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            b"type\0".as_ptr() as *const c_char,
            js_int32(attr.gl_type as i32),
        );
        obj
    }

    unsafe extern "C" fn gl_get_extension(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_null();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let cstr = qjs::js_to_cstring(ctx, args[0]);
        if cstr.is_null() {
            return js_null();
        }
        let name = CStr::from_ptr(cstr).to_bytes();
        let is_uint_ext = name.eq_ignore_ascii_case(b"OES_element_index_uint");
        let is_vao_ext = name.eq_ignore_ascii_case(b"OES_vertex_array_object");
        qjs::JS_FreeCString(ctx, cstr);
        if is_vao_ext {
            let ext = qjs::JS_NewObject(ctx);
            if ext.is_exception() {
                return ext;
            }
            macro_rules! ext_fn {
                ($name:literal, $func:expr, $argc:expr) => {{
                    let k = concat!($name, "\0");
                    let f = qjs::JS_NewCFunction2(
                        ctx,
                        Some($func),
                        k.as_ptr() as *const c_char,
                        $argc,
                        qjs::JS_CFUNC_GENERIC,
                        0,
                    );
                    let _ = qjs::JS_SetPropertyStr(ctx, ext, k.as_ptr() as *const c_char, f);
                }};
            }
            ext_fn!("createVertexArrayOES", gl_create_vertex_array_oes, 0);
            ext_fn!("bindVertexArrayOES", gl_bind_vertex_array_oes, 1);
            ext_fn!("deleteVertexArrayOES", gl_delete_vertex_array_oes, 1);
            ext_fn!("isVertexArrayOES", gl_is_vertex_array_oes, 1);
            return ext;
        }
        if is_uint_ext {
            return qjs::JS_NewObject(ctx);
        }
        js_null()
    }

    unsafe extern "C" fn gl_get_context_attributes(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let obj = qjs::JS_NewObject(ctx);
        if obj.is_exception() {
            return obj;
        }
        let js_true = qjs::JSValue {
            u: qjs::JSValueUnion { int32: 1 },
            tag: qjs::JS_TAG_BOOL,
        };
        let js_false = qjs::JSValue {
            u: qjs::JSValueUnion { int32: 0 },
            tag: qjs::JS_TAG_BOOL,
        };
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"alpha\0".as_ptr() as *const c_char, js_true);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"antialias\0".as_ptr() as *const c_char, js_false);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"depth\0".as_ptr() as *const c_char, js_false);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"stencil\0".as_ptr() as *const c_char, js_true);
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            b"premultipliedAlpha\0".as_ptr() as *const c_char,
            js_true,
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            b"preserveDrawingBuffer\0".as_ptr() as *const c_char,
            js_false,
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            b"powerPreference\0".as_ptr() as *const c_char,
            qjs::JS_NewStringLen(ctx, b"default\0".as_ptr() as *const c_char, 7),
        );
        obj
    }

    // Build the gl object.
    let gl = qjs::JS_NewObject(ctx);
    if gl.is_exception() {
        return gl;
    }

    // A small set of WebGL constants Pixi-like stacks commonly touch.
    macro_rules! gl_const {
        ($name:literal, $val:expr) => {{
            let k = concat!($name, "\0");
            let _ = qjs::JS_SetPropertyStr(ctx, gl, k.as_ptr() as *const c_char, js_int32($val));
        }};
    }

    gl_const!("NO_ERROR", 0);
    gl_const!("INVALID_ENUM", 0x0500);
    gl_const!("INVALID_VALUE", 0x0501);
    gl_const!("INVALID_OPERATION", 0x0502);
    gl_const!("OUT_OF_MEMORY", 0x0505);

    gl_const!("ARRAY_BUFFER", 0x8892);
    gl_const!("ELEMENT_ARRAY_BUFFER", 0x8893);
    gl_const!("STATIC_DRAW", 0x88E4);
    gl_const!("DYNAMIC_DRAW", 0x88E8);

    gl_const!("TEXTURE_2D", 0x0DE1);
    gl_const!("TEXTURE0", 0x84C0);
    gl_const!("FRAMEBUFFER", 0x8D40);
    gl_const!("DRAW_FRAMEBUFFER", 0x8CA9);
    gl_const!("FRAMEBUFFER_BINDING", 0x8CA6);
    gl_const!("DRAW_FRAMEBUFFER_BINDING", 0x8CA6);
    gl_const!("SAMPLES", 0x80A9);
    gl_const!("RGBA", 0x1908);
    gl_const!("RGB", 0x1907);
    gl_const!("UNSIGNED_BYTE", 0x1401);
    gl_const!("UNSIGNED_SHORT", 0x1403);
    gl_const!("FLOAT", 0x1406);
    gl_const!("UNPACK_ALIGNMENT", 0x0CF5);

    gl_const!("VERTEX_SHADER", 0x8B31);
    gl_const!("FRAGMENT_SHADER", 0x8B30);
    gl_const!("COMPILE_STATUS", 0x8B81);
    gl_const!("LINK_STATUS", 0x8B82);
    gl_const!("ACTIVE_UNIFORMS", 0x8B86);
    gl_const!("ACTIVE_ATTRIBUTES", 0x8B89);

    gl_const!("TRIANGLES", 0x0004);
    gl_const!("BLEND", 0x0BE2);
    gl_const!("SCISSOR_TEST", 0x0C11);
    gl_const!("CULL_FACE", 0x0B44);
    gl_const!("CULL_FACE_MODE", 0x0B45);
    gl_const!("FRONT_FACE", 0x0B46);
    gl_const!("DEPTH_TEST", 0x0B71);
    gl_const!("LEQUAL", 0x0203);
    gl_const!("LESS", 0x0201);
    gl_const!("FRONT", 0x0404);
    gl_const!("BACK", 0x0405);
    gl_const!("FRONT_AND_BACK", 0x0408);
    gl_const!("CW", 0x0900);
    gl_const!("CCW", 0x0901);

    gl_const!("COLOR_BUFFER_BIT", 0x4000);

    gl_const!("ONE", 1);
    gl_const!("ZERO", 0);
    gl_const!("SRC_COLOR", 0x0300);
    gl_const!("DST_COLOR", 0x0306);
    gl_const!("ONE_MINUS_DST_COLOR", 0x0307);
    gl_const!("ONE_MINUS_SRC_COLOR", 0x0301);
    gl_const!("DST_ALPHA", 0x0304);
    gl_const!("ONE_MINUS_DST_ALPHA", 0x0305);
    gl_const!("ONE_MINUS_SRC_ALPHA", 0x0303);
    gl_const!("SRC_ALPHA", 0x0302);
    gl_const!("BLEND_EQUATION", 0x8009);
    gl_const!("BLEND_EQUATION_RGB", 0x8009);
    gl_const!("BLEND_EQUATION_ALPHA", 0x883D);
    gl_const!("BLEND_SRC_RGB", 0x80C9);
    gl_const!("BLEND_DST_RGB", 0x80C8);
    gl_const!("BLEND_SRC_ALPHA", 0x80CB);
    gl_const!("BLEND_DST_ALPHA", 0x80CA);
    gl_const!("FUNC_ADD", 0x8006);
    gl_const!("FUNC_SUBTRACT", 0x800A);
    gl_const!("FUNC_REVERSE_SUBTRACT", 0x800B);

    gl_const!("MAX_TEXTURE_SIZE", 0x0D33);
    gl_const!("MAX_TEXTURE_IMAGE_UNITS", 0x8872);
    gl_const!("MAX_VERTEX_ATTRIBS", 0x8869);
    gl_const!("VERSION", 0x1F02);
    gl_const!("VENDOR", 0x1F00);
    gl_const!("RENDERER", 0x1F01);

    // Methods: mostly no-op, but creation returns handles and getParameter/getError return useful values.
    macro_rules! gl_fn {
        ($name:literal, $func:expr, $argc:expr) => {{
            let k = concat!($name, "\0");
            let f = qjs::JS_NewCFunction2(
                ctx,
                Some($func),
                k.as_ptr() as *const c_char,
                $argc,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ = qjs::JS_SetPropertyStr(ctx, gl, k.as_ptr() as *const c_char, f);
        }};
    }

    gl_fn!("getError", gl_get_error, 0);
    gl_fn!("getContextAttributes", gl_get_context_attributes, 0);
    gl_fn!("getParameter", gl_get_parameter, 1);
    gl_fn!("getExtension", gl_get_extension, 1);
    gl_fn!("getSupportedExtensions", gl_get_supported_extensions, 0);
    gl_fn!("getShaderPrecisionFormat", gl_get_shader_precision_format, 2);
    gl_fn!("isContextLost", gl_return_false, 0);

    // Object creation helpers
    gl_fn!("createBuffer", gl_create_handle, 0);
    gl_fn!("createTexture", gl_create_handle, 0);
    gl_fn!("createShader", gl_create_shader, 1);
    gl_fn!("createProgram", gl_create_handle, 0);
    gl_fn!("createVertexArray", gl_create_vertex_array_oes, 0);
    gl_fn!("getUniformLocation", gl_get_uniform_location, 2);

    // Generic no-ops (extend as Pixi tells us what it needs)
    gl_fn!("bindBuffer", gl_bind_buffer, 2);
    gl_fn!("bindVertexArray", gl_bind_vertex_array_oes, 1);
    gl_fn!("bufferData", gl_buffer_data, 3);
    gl_fn!("bufferSubData", gl_buffer_sub_data, 3);
    gl_fn!("bindTexture", gl_bind_texture, 2);
    gl_fn!("bindFramebuffer", gl_noop, 2);
    gl_fn!("activeTexture", gl_active_texture, 1);
    gl_fn!("texParameteri", gl_tex_parameter_i, 3);
    gl_fn!("texImage2D", gl_tex_image_2d, 9);
    gl_fn!("texSubImage2D", gl_tex_sub_image_2d, 9);
    gl_fn!("pixelStorei", gl_pixel_store_i, 2);
    gl_fn!("shaderSource", gl_shader_source, 2);
    gl_fn!("compileShader", gl_noop, 1);
    gl_fn!("attachShader", gl_attach_shader, 2);
    gl_fn!("detachShader", gl_detach_shader, 2);
    gl_fn!("linkProgram", gl_link_program, 1);
    gl_fn!("bindAttribLocation", gl_bind_attrib_location, 3);
    gl_fn!("useProgram", gl_noop, 1);
    gl_fn!("deleteShader", gl_noop, 1);
    gl_fn!("deleteProgram", gl_noop, 1);
    gl_fn!("deleteVertexArray", gl_delete_vertex_array_oes, 1);
    gl_fn!("isVertexArray", gl_is_vertex_array_oes, 1);
    gl_fn!("enableVertexAttribArray", gl_enable_vertex_attrib_array, 1);
    gl_fn!("disableVertexAttribArray", gl_disable_vertex_attrib_array, 1);
    gl_fn!("vertexAttribPointer", gl_vertex_attrib_pointer, 6);
    gl_fn!("uniform1i", gl_noop, 2);
    gl_fn!("uniform1f", gl_noop, 2);
    gl_fn!("uniform2f", gl_noop, 3);
    gl_fn!("uniform4f", gl_noop, 5);
    gl_fn!("uniformMatrix3fv", gl_uniform_matrix3fv, 3);
    gl_fn!("uniformMatrix4fv", gl_uniform_matrix4fv, 3);
    gl_fn!("viewport", gl_viewport, 4);
    gl_fn!("scissor", gl_noop, 4);
    gl_fn!("enable", gl_enable, 1);
    gl_fn!("disable", gl_disable, 1);
    gl_fn!("isEnabled", gl_is_enabled, 1);
    gl_fn!("frontFace", gl_front_face, 1);
    gl_fn!("cullFace", gl_cull_face, 1);
    gl_fn!("depthMask", gl_noop, 1);
    gl_fn!("depthFunc", gl_noop, 1);
    gl_fn!("depthRange", gl_noop, 2);
    gl_fn!("clearDepth", gl_noop, 1);
    gl_fn!("blendFunc", gl_blend_func, 2);
    gl_fn!("blendFuncSeparate", gl_blend_func_separate, 4);
    gl_fn!("blendEquation", gl_blend_equation, 1);
    gl_fn!("blendEquationSeparate", gl_blend_equation_separate, 2);
    gl_fn!("clearColor", gl_clear_color, 4);
    gl_fn!("clear", gl_clear, 1);
    gl_fn!("drawElements", gl_draw_elements, 4);
    gl_fn!("drawArrays", gl_draw_arrays, 3);
    gl_fn!("flush", gl_noop, 0);

    // Minimal success-y queries
    gl_fn!("getShaderParameter", gl_return_true, 2);
    gl_fn!("getProgramParameter", gl_get_program_parameter, 2);
    gl_fn!("getActiveAttrib", gl_get_active_attrib, 2);
    gl_fn!("getAttribLocation", gl_get_attrib_location, 2);
    gl_fn!("getShaderInfoLog", gl_return_null, 1);
    gl_fn!("getProgramInfoLog", gl_return_null, 1);

    let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, gl);
    // Return a borrowed handle from global
    qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char)
}

pub(crate) unsafe fn ensure_global_document(
    ctx: *mut qjs::JSContext,
    global: qjs::JSValue,
    gl_obj: qjs::JSValue,
) -> qjs::JSValue {
    let key = b"document\0";
    let existing = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    if !existing.is_exception() && existing.tag != qjs::JS_TAG_UNDEFINED {
        return existing;
    }
    qjs::js_free_value(ctx, existing);

    unsafe extern "C" fn doc_create_element(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JS_NewObject(ctx);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let cstr = qjs::js_to_cstring(ctx, args[0]);
        if cstr.is_null() {
            return qjs::JS_NewObject(ctx);
        }
        let tag = CStr::from_ptr(cstr).to_bytes();
        qjs::JS_FreeCString(ctx, cstr);

        // We only special-case canvas for now.
        if tag.eq_ignore_ascii_case(b"canvas") {
            // Canvas object with getContext().
            unsafe extern "C" fn canvas_get_context(
                ctx: *mut qjs::JSContext,
                this_val: qjs::JSValueConst,
                argc: c_int,
                argv: *const qjs::JSValueConst,
            ) -> qjs::JSValue {
                // If the caller asked for "2d", return null for now (explicitly not supported).
                if !argv.is_null() && argc >= 1 {
                    let args = core::slice::from_raw_parts(argv, argc as usize);
                    let cstr = qjs::js_to_cstring(ctx, args[0]);
                    if !cstr.is_null() {
                        let kind = CStr::from_ptr(cstr).to_bytes();
                        qjs::JS_FreeCString(ctx, cstr);
                        if WEBGL_LOG_GET_CONTEXT.fetch_add(1, Ordering::Relaxed) < 12 {
                            log_str("qjs-webgl: canvas.getContext kind=");
                            log_bytes(kind);
                            log_str(" argc=");
                            log_usize_dec(argc.max(0) as usize);
                            log_str("\n");
                        }
                        if kind.eq_ignore_ascii_case(b"2d") {
                            return js_null();
                        }

                        // We currently only model a very small WebGL 1-ish subset.
                        // Returning a non-null object for "webgl2" causes libraries like Pixi
                        // to take WebGL2 code paths (VAOs, UBOs, etc.) that our shim does not
                        // implement, often resulting in a blank scene.
                        if kind.eq_ignore_ascii_case(b"webgl2") {
                            return js_null();
                        }

                        // Pixi (and friends) do feature probes with a one-arg
                        // getContext("webgl") call and then await media events.
                        // We don't model that event loop, so treat one-arg webgl/webgl2
                        // as "unsupported" to avoid stalling module initialization.
                        if (kind.eq_ignore_ascii_case(b"webgl")
                            || kind.eq_ignore_ascii_case(b"webgl2"))
                            && argc < 2
                        {
                            // Escape hatch for explicit smokes/tests.
                            let global = qjs::JS_GetGlobalObject(ctx);
                            let force = qjs::JS_GetPropertyStr(
                                ctx,
                                global,
                                b"__trueos_webgl_force\0".as_ptr() as *const c_char,
                            );
                            qjs::js_free_value(ctx, global);
                            let mut f: f64 = 0.0;
                            let forced = (!force.is_exception())
                                && (qjs::JS_ToFloat64(ctx, &mut f as *mut f64, force) == 0)
                                && (f != 0.0);
                            qjs::js_free_value(ctx, force);
                            if forced {
                                // allow
                            } else {
                            return js_null();
                            }
                        }
                    }
                }

                // For actual renderer creation paths (usually passing options),
                // return the shared singleton.
                let global = qjs::JS_GetGlobalObject(ctx);
                let gl = qjs::JS_GetPropertyStr(
                    ctx,
                    global,
                    b"__trueos_gl\0".as_ptr() as *const c_char,
                );
                qjs::js_free_value(ctx, global);
                if gl.is_exception() {
                    return js_null();
                }

                // Store last_context on the canvas for debugging.
                let _ = qjs::JS_SetPropertyStr(
                    ctx,
                    this_val,
                    b"__trueos_last_context\0".as_ptr() as *const c_char,
                    qjs::js_dup_value(ctx, gl),
                );

                gl
            }

            let canvas = qjs::browser::make_dom_like_element(ctx);
            if canvas.is_exception() {
                return canvas;
            }

            // width/height default
            let _ = qjs::JS_SetPropertyStr(ctx, canvas, b"width\0".as_ptr() as *const c_char, js_int32(1));
            let _ = qjs::JS_SetPropertyStr(ctx, canvas, b"height\0".as_ptr() as *const c_char, js_int32(1));

            let name = b"getContext\0";
            let f = qjs::JS_NewCFunction2(
                ctx,
                Some(canvas_get_context),
                name.as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ = qjs::JS_SetPropertyStr(ctx, canvas, name.as_ptr() as *const c_char, f);

            return canvas;
        }

        qjs::browser::make_dom_like_element(ctx)
    }

    // Create document object
    let doc = qjs::JS_NewObject(ctx);
    if doc.is_exception() {
        return doc;
    }

    // document.body placeholder
    let body = qjs::browser::make_dom_like_element(ctx);
    if !body.is_exception() {
        let _ = qjs::JS_SetPropertyStr(ctx, body, b"width\0".as_ptr() as *const c_char, js_int32(1280));
        let _ = qjs::JS_SetPropertyStr(ctx, body, b"height\0".as_ptr() as *const c_char, js_int32(800));
        let _ = qjs::JS_SetPropertyStr(ctx, doc, b"body\0".as_ptr() as *const c_char, body);
    } else {
        qjs::js_free_value(ctx, body);
    }

    qjs::browser::ensure_global_event_target_stubs(ctx, doc);

    // document.createElement
    let name = b"createElement\0";
    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(doc_create_element),
        name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, doc, name.as_ptr() as *const c_char, f);

    // Also expose the gl object in case libraries probe for it.
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        doc,
        b"__trueos_gl\0".as_ptr() as *const c_char,
        qjs::js_dup_value(ctx, gl_obj),
    );

    let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, doc);
    qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char)
}
