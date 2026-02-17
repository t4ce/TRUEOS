extern crate alloc;

use alloc::vec::Vec;
use alloc::vec;
use core::ffi::{c_char, c_int, CStr};
use spin::Mutex;

use crate as qjs;
use crate::cmd_stream;

extern "C" {
}

const MAX_ATTRS: usize = 16;

const GL_FLOAT: u32 = 0x1406;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_UNSIGNED_SHORT: u32 = 0x1403;
const GL_UNSIGNED_INT: u32 = 0x1405;
const GL_RGBA: u32 = 0x1908;

const GL_TRIANGLES: u32 = 0x0004;
const GL_TRIANGLE_STRIP: u32 = 0x0005;
const GL_TRIANGLE_FAN: u32 = 0x0006;

const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_ELEMENT_ARRAY_BUFFER: u32 = 0x8893;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_UNPACK_ALIGNMENT: u32 = 0x0CF5;

const GL_BLEND: u32 = 0x0BE2;
const GL_COLOR_BUFFER_BIT: u32 = 0x0000_4000;

const GL_COMPILE_STATUS: u32 = 0x8B81;
const GL_LINK_STATUS: u32 = 0x8B82;
const GL_ACTIVE_ATTRIBUTES: u32 = 0x8B89;
const GL_ACTIVE_UNIFORMS: u32 = 0x8B86;

const GL_VERSION: u32 = 0x1F02;
const GL_RENDERER: u32 = 0x1F01;
const GL_VENDOR: u32 = 0x1F00;
const GL_MAX_VERTEX_ATTRIBS: u32 = 0x8869;
const GL_MAX_TEXTURE_IMAGE_UNITS: u32 = 0x8872;
const GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS: u32 = 0x8B4D;
const GL_STENCIL_BITS: u32 = 0x0D57;
const GL_FRAGMENT_SHADER: u32 = 0x8B30;
const GL_VERTEX_SHADER: u32 = 0x8B31;
const GL_HIGH_FLOAT: u32 = 0x8DF2;
const GL_MEDIUM_FLOAT: u32 = 0x8DF1;
const GL_LOW_FLOAT: u32 = 0x8DF0;
const GL_HIGH_INT: u32 = 0x8DF5;
const GL_MEDIUM_INT: u32 = 0x8DF4;
const GL_LOW_INT: u32 = 0x8DF3;
const GL_FLOAT_VEC2: u32 = 0x8B50;
const GL_FLOAT_VEC3: u32 = 0x8B51;
const GL_FLOAT_VEC4: u32 = 0x8B52;
const GL_INT: u32 = 0x1404;
const GL_INT_VEC2: u32 = 0x8B53;
const GL_INT_VEC3: u32 = 0x8B54;
const GL_INT_VEC4: u32 = 0x8B55;
const GL_BOOL: u32 = 0x8B56;
const GL_BOOL_VEC2: u32 = 0x8B57;
const GL_BOOL_VEC3: u32 = 0x8B58;
const GL_BOOL_VEC4: u32 = 0x8B59;
const GL_FLOAT_MAT2: u32 = 0x8B5A;
const GL_FLOAT_MAT3: u32 = 0x8B5B;
const GL_FLOAT_MAT4: u32 = 0x8B5C;
const GL_SAMPLER_2D: u32 = 0x8B5E;
const GL_SAMPLER_CUBE: u32 = 0x8B60;
const GL_SAMPLER_2D_ARRAY: u32 = 0x8DC1;
const GL_UNSIGNED_INT_VEC2: u32 = 0x8DC6;
const GL_UNSIGNED_INT_VEC3: u32 = 0x8DC7;
const GL_UNSIGNED_INT_VEC4: u32 = 0x8DC8;
const GL_INT_SAMPLER_2D: u32 = 0x8DCA;
const GL_INT_SAMPLER_CUBE: u32 = 0x8DCC;
const GL_INT_SAMPLER_2D_ARRAY: u32 = 0x8DCF;
const GL_UNSIGNED_INT_SAMPLER_2D: u32 = 0x8DD2;
const GL_UNSIGNED_INT_SAMPLER_CUBE: u32 = 0x8DD4;
const GL_UNSIGNED_INT_SAMPLER_2D_ARRAY: u32 = 0x8DD7;

#[derive(Clone, Copy)]
struct AttribState {
    enabled: bool,
    size: i32,
    type_enum: u32,
    normalized: bool,
    stride: i32,
    offset: usize,
    buffer_id: u32,
}

impl Default for AttribState {
    fn default() -> Self {
        Self {
            enabled: false,
            size: 4,
            type_enum: GL_FLOAT,
            normalized: false,
            stride: 0,
            offset: 0,
            buffer_id: 0,
        }
    }
}

const fn attrib_default() -> AttribState {
    AttribState {
        enabled: false,
        size: 4,
        type_enum: GL_FLOAT,
        normalized: false,
        stride: 0,
        offset: 0,
        buffer_id: 0,
    }
}

struct ProgramState {
    attrib_names: Vec<Vec<u8>>,
    uniform_names: Vec<Vec<u8>>,
    uniform_mat3: Vec<Option<[f32; 9]>>,
    active_attribs: Vec<(Vec<u8>, u32, i32)>,
    active_uniforms: Vec<(Vec<u8>, u32, i32)>,
    attached_vertex_shader: u32,
    attached_fragment_shader: u32,
    linked: bool,
}

impl ProgramState {
    fn new() -> Self {
        Self {
            attrib_names: Vec::new(),
            uniform_names: Vec::new(),
            uniform_mat3: Vec::new(),
            active_attribs: Vec::new(),
            active_uniforms: Vec::new(),
            attached_vertex_shader: 0,
            attached_fragment_shader: 0,
            linked: false,
        }
    }
}

struct ShaderState {
    shader_type: u32,
    source: Vec<u8>,
    compiled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct VertexDecodeCacheKey {
    buffer_id: u32,
    buffer_rev: u32,
    stride: usize,
    offset: usize,
    size: i32,
    type_enum: u32,
}

struct VertexDecodeCache {
    key: Option<VertexDecodeCacheKey>,
    indices: Vec<u32>,
    xy: Vec<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct IndexExpandCacheKey {
    elem_buffer_id: u32,
    elem_buffer_rev: u32,
    mode: u32,
    count: usize,
    index_type: u32,
    index_offset: usize,
}

struct IndexExpandCache {
    key: Option<IndexExpandCacheKey>,
    tri: Vec<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PackedVertexCacheKey {
    buffer_id: u32,
    buffer_rev: u32,
    stride: usize,
    offset: usize,
    size: i32,
    type_enum: u32,
    current_program: u32,
    viewport_w: i32,
    viewport_h: i32,
    transform_epoch: u32,
    indices_len: usize,
    indices_hash: u32,
    color_buffer_id: u32,
    color_buffer_rev: u32,
    color_stride: usize,
    color_offset: usize,
    color_size: i32,
    color_type_enum: u32,
    uv_buffer_id: u32,
    uv_buffer_rev: u32,
    uv_stride: usize,
    uv_offset: usize,
    uv_size: i32,
    uv_type_enum: u32,
    texture_id: u32,
    texture_rev: u32,
}

struct TextureState {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    rev: u32,
}

struct GlState {
    next_handle: u32,
    buffers: Vec<Option<Vec<u8>>>,
    textures: Vec<Option<TextureState>>,
    shaders: Vec<Option<ShaderState>>,
    programs: Vec<Option<ProgramState>>,
    current_array_buffer: u32,
    current_element_array_buffer: u32,
    current_texture_2d: u32,
    current_program: u32,
    attribs: [AttribState; MAX_ATTRS],
    buffer_revs: Vec<u32>,
    texture_revs: Vec<u32>,
    vertex_decode_cache: VertexDecodeCache,
    index_expand_cache: IndexExpandCache,
    packed_vertex_cache_key: Option<PackedVertexCacheKey>,
    packed_vertex_cache: Vec<u8>,
    clear_rgb: u32,
    viewport_w: i32,
    viewport_h: i32,
    blend_enabled: bool,
    transform_epoch: u32,
    unpack_alignment: i32,
    viewport_dirty: bool,
    clear_dirty: bool,
    blend_dirty: bool,
    frame_open: bool,
}

impl GlState {
    const fn new() -> Self {
        Self {
            next_handle: 1,
            buffers: Vec::new(),
            textures: Vec::new(),
            shaders: Vec::new(),
            programs: Vec::new(),
            current_array_buffer: 0,
            current_element_array_buffer: 0,
            current_texture_2d: 0,
            current_program: 0,
            attribs: [attrib_default(); MAX_ATTRS],
            buffer_revs: Vec::new(),
            texture_revs: Vec::new(),
            vertex_decode_cache: VertexDecodeCache {
                key: None,
                indices: Vec::new(),
                xy: Vec::new(),
            },
            index_expand_cache: IndexExpandCache {
                key: None,
                tri: Vec::new(),
            },
            packed_vertex_cache_key: None,
            packed_vertex_cache: Vec::new(),
            clear_rgb: 0x12161d,
            viewport_w: 1280,
            viewport_h: 800,
            blend_enabled: false,
            transform_epoch: 1,
            unpack_alignment: 4,
            viewport_dirty: true,
            clear_dirty: true,
            blend_dirty: true,
            frame_open: false,
        }
    }

    fn alloc_handle(&mut self) -> u32 {
        let id = self.next_handle;
        self.next_handle = self.next_handle.saturating_add(1);
        id
    }
}

static GL_STATE: Mutex<GlState> = Mutex::new(GlState::new());

#[inline]
fn js_bool(v: bool) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: if v { 1 } else { 0 } },
        tag: qjs::JS_TAG_BOOL,
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
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

unsafe fn js_get_f64(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<f64> {
    let mut out = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut out as *mut f64, v) == 0 {
        Some(out)
    } else {
        None
    }
}

unsafe fn js_get_arraybuffer_view(
    ctx: *mut qjs::JSContext,
    val: qjs::JSValueConst,
) -> Option<(*const u8, usize)> {
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
            let end = byte_off.saturating_add(byte_len);
            if end <= total {
                return Some((ptr.add(byte_off) as *const u8, byte_len));
            }
        }
    } else {
        qjs::js_free_value(ctx, ab);
    }
    let mut total: usize = 0;
    let ptr = qjs::JS_GetArrayBuffer(ctx, &mut total as *mut usize, val);
    if !ptr.is_null() {
        Some((ptr as *const u8, total))
    } else {
        None
    }
}

unsafe fn js_get_obj_u32(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    name: *const c_char,
) -> Option<u32> {
    if obj.tag != qjs::JS_TAG_OBJECT {
        return None;
    }
    let prop = qjs::JS_GetPropertyStr(ctx, obj, name);
    if prop.is_exception() || prop.tag == qjs::JS_TAG_UNDEFINED {
        qjs::js_free_value(ctx, prop);
        return None;
    }
    let out = js_get_f64(ctx, prop).map(|x| x.max(0.0) as u32);
    qjs::js_free_value(ctx, prop);
    out
}

unsafe fn js_get_obj_arraybuffer_view(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    name: *const c_char,
) -> Option<(*const u8, usize)> {
    if obj.tag != qjs::JS_TAG_OBJECT {
        return None;
    }
    let prop = qjs::JS_GetPropertyStr(ctx, obj, name);
    if prop.is_exception() || prop.tag == qjs::JS_TAG_UNDEFINED {
        qjs::js_free_value(ctx, prop);
        return None;
    }
    let out = js_get_arraybuffer_view(ctx, prop);
    qjs::js_free_value(ctx, prop);
    out
}

unsafe fn js_get_handle_id(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<u32> {
    if v.tag == qjs::JS_TAG_NULL || v.tag == qjs::JS_TAG_UNDEFINED {
        return Some(0);
    }
    if v.tag != qjs::JS_TAG_OBJECT {
        return None;
    }
    let prop = qjs::JS_GetPropertyStr(ctx, v, b"__trueos_id\0".as_ptr() as *const c_char);
    if prop.is_exception() || prop.tag == qjs::JS_TAG_UNDEFINED {
        qjs::js_free_value(ctx, prop);
        return None;
    }
    let out = js_get_f64(ctx, prop).map(|x| x.max(0.0) as u32);
    qjs::js_free_value(ctx, prop);
    out
}

unsafe fn js_new_handle_obj(ctx: *mut qjs::JSContext, id: u32) -> qjs::JSValue {
    let o = qjs::JS_NewObject(ctx);
    if o.is_exception() {
        return o;
    }
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        o,
        b"__trueos_id\0".as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, id as f64),
    );
    o
}

fn set_i32_const(ctx: *mut qjs::JSContext, obj: qjs::JSValue, name: &'static [u8], v: i32) {
    let _ = unsafe { qjs::JS_SetPropertyStr(ctx, obj, name.as_ptr() as *const c_char, js_int32(v)) };
}

fn buffer_slot_mut(st: &mut GlState, id: u32) -> Option<&mut Vec<u8>> {
    if id == 0 {
        return None;
    }
    let idx = (id - 1) as usize;
    if idx >= st.buffers.len() {
        return None;
    }
    st.buffers[idx].as_mut()
}

fn texture_slot_mut(st: &mut GlState, id: u32) -> Option<&mut TextureState> {
    if id == 0 {
        return None;
    }
    let idx = (id - 1) as usize;
    if idx >= st.textures.len() {
        return None;
    }
    st.textures[idx].as_mut()
}

fn texture_slot(st: &GlState, id: u32) -> Option<&TextureState> {
    if id == 0 {
        return None;
    }
    let idx = (id - 1) as usize;
    if idx >= st.textures.len() {
        return None;
    }
    st.textures[idx].as_ref()
}

fn program_slot_mut(st: &mut GlState, id: u32) -> Option<&mut ProgramState> {
    if id == 0 {
        return None;
    }
    let idx = (id - 1) as usize;
    if idx >= st.programs.len() {
        return None;
    }
    st.programs[idx].as_mut()
}

fn shader_slot_mut(st: &mut GlState, id: u32) -> Option<&mut ShaderState> {
    if id == 0 {
        return None;
    }
    let idx = (id - 1) as usize;
    if idx >= st.shaders.len() {
        return None;
    }
    st.shaders[idx].as_mut()
}

fn shader_slot(st: &GlState, id: u32) -> Option<&ShaderState> {
    if id == 0 {
        return None;
    }
    let idx = (id - 1) as usize;
    if idx >= st.shaders.len() {
        return None;
    }
    st.shaders[idx].as_ref()
}

fn glsl_type_to_enum(tok: &[u8]) -> u32 {
    if tok == b"float" {
        GL_FLOAT
    } else if tok == b"vec2" {
        GL_FLOAT_VEC2
    } else if tok == b"vec3" {
        GL_FLOAT_VEC3
    } else if tok == b"vec4" {
        GL_FLOAT_VEC4
    } else if tok == b"int" {
        GL_INT
    } else if tok == b"ivec2" {
        GL_INT_VEC2
    } else if tok == b"ivec3" {
        GL_INT_VEC3
    } else if tok == b"ivec4" {
        GL_INT_VEC4
    } else if tok == b"uint" {
        GL_UNSIGNED_INT
    } else if tok == b"uvec2" {
        GL_UNSIGNED_INT_VEC2
    } else if tok == b"uvec3" {
        GL_UNSIGNED_INT_VEC3
    } else if tok == b"uvec4" {
        GL_UNSIGNED_INT_VEC4
    } else if tok == b"bool" {
        GL_BOOL
    } else if tok == b"bvec2" {
        GL_BOOL_VEC2
    } else if tok == b"bvec3" {
        GL_BOOL_VEC3
    } else if tok == b"bvec4" {
        GL_BOOL_VEC4
    } else if tok == b"mat2" {
        GL_FLOAT_MAT2
    } else if tok == b"mat3" {
        GL_FLOAT_MAT3
    } else if tok == b"mat4" {
        GL_FLOAT_MAT4
    } else if tok == b"sampler2D" {
        GL_SAMPLER_2D
    } else if tok == b"samplerCube" {
        GL_SAMPLER_CUBE
    } else if tok == b"sampler2DArray" {
        GL_SAMPLER_2D_ARRAY
    } else {
        GL_FLOAT
    }
}

fn is_glsl_qualifier(tok: &str) -> bool {
    matches!(
        tok,
        "lowp"
            | "mediump"
            | "highp"
            | "flat"
            | "smooth"
            | "noperspective"
            | "centroid"
            | "invariant"
            | "precise"
    )
}

fn sanitize_glsl_name(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for b in name {
        if *b == b';' || *b == b',' {
            break;
        }
        if *b == b'[' {
            break;
        }
        out.push(*b);
    }
    out
}

fn scan_glsl_decl(src: &[u8], key: &[u8], out: &mut Vec<(Vec<u8>, u32, i32)>) {
    let Ok(s) = core::str::from_utf8(src) else {
        return;
    };
    let Ok(key_s) = core::str::from_utf8(key) else {
        return;
    };
    for raw in s.lines() {
        let line = raw.trim();
        if !line.as_bytes().starts_with(key) {
            continue;
        }
        let Some(mut tail) = line.strip_prefix(key_s) else {
            continue;
        };
        tail = tail.trim_start();
        let mut parts = tail.split_whitespace();
        let mut type_tok = parts.next().unwrap_or("");
        while is_glsl_qualifier(type_tok) {
            type_tok = parts.next().unwrap_or("");
            if type_tok.is_empty() {
                break;
            }
        }
        let name_tok = parts.next().unwrap_or("");
        if type_tok.is_empty() || name_tok.is_empty() {
            continue;
        }
        if name_tok.is_empty() {
            continue;
        }
        let n = sanitize_glsl_name(name_tok.as_bytes());
        if n.is_empty() {
            continue;
        }
        if out.iter().any(|(x, _, _)| *x == n) {
            continue;
        }
        out.push((n, glsl_type_to_enum(type_tok.as_bytes()), 1));
    }
}

unsafe fn uniform_loc_index(ctx: *mut qjs::JSContext, loc_obj: qjs::JSValueConst) -> Option<usize> {
    if loc_obj.tag != qjs::JS_TAG_OBJECT {
        return None;
    }
    let v = qjs::JS_GetPropertyStr(ctx, loc_obj, b"__u\0".as_ptr() as *const c_char);
    let loc = js_get_f64(ctx, v).map(|x| x.max(0.0) as usize);
    qjs::js_free_value(ctx, v);
    loc
}

unsafe fn uniform_loc_program(ctx: *mut qjs::JSContext, loc_obj: qjs::JSValueConst) -> u32 {
    if loc_obj.tag != qjs::JS_TAG_OBJECT {
        return 0;
    }
    let v = qjs::JS_GetPropertyStr(ctx, loc_obj, b"__p\0".as_ptr() as *const c_char);
    let prog = js_get_f64(ctx, v).map(|x| x.max(0.0) as u32).unwrap_or(0);
    qjs::js_free_value(ctx, v);
    prog
}

unsafe fn with_uniform_loc<F>(
    ctx: *mut qjs::JSContext,
    loc_obj: qjs::JSValueConst,
    mut f: F,
) -> qjs::JSValue
where
    F: FnMut(&mut ProgramState, usize),
{
    let Some(loc) = uniform_loc_index(ctx, loc_obj) else {
        return qjs::JSValue::undefined();
    };
    let mut st = GL_STATE.lock();
    let prog_id = {
        let p = uniform_loc_program(ctx, loc_obj);
        if p != 0 { p } else { st.current_program }
    };
    let Some(p) = program_slot_mut(&mut st, prog_id) else {
        return qjs::JSValue::undefined();
    };
    if loc >= p.uniform_names.len() {
        return qjs::JSValue::undefined();
    }
    f(p, loc);
    qjs::JSValue::undefined()
}

unsafe fn gl_uniform_store_noop(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let loc_obj = args[0];
    with_uniform_loc(ctx, loc_obj, |_p, _loc| {})
}

fn read_index(src: &[u8], type_enum: u32, offset: usize, i: usize) -> Option<u32> {
    match type_enum {
        GL_UNSIGNED_BYTE => src.get(offset + i).copied().map(|x| x as u32),
        GL_UNSIGNED_SHORT => {
            let p = offset + i * 2;
            let bytes = src.get(p..p + 2)?;
            Some(u16::from_le_bytes([bytes[0], bytes[1]]) as u32)
        }
        GL_UNSIGNED_INT => {
            let p = offset + i * 4;
            let bytes = src.get(p..p + 4)?;
            Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        _ => None,
    }
}

fn hash_u32_slice(values: &[u32]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for v in values {
        h ^= *v;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

fn mat3_apply(m: &[f32; 9], x: f32, y: f32) -> (f32, f32) {
    let ox = m[0] * x + m[3] * y + m[6];
    let oy = m[1] * x + m[4] * y + m[7];
    let ow = m[2] * x + m[5] * y + m[8];
    if ow != 0.0 {
        (ox / ow, oy / ow)
    } else {
        (ox, oy)
    }
}

fn find_uniform_mat3(st: &GlState, prog_id: u32, names: &[&[u8]]) -> Option<[f32; 9]> {
    if prog_id == 0 {
        return None;
    }
    let idx = (prog_id - 1) as usize;
    let Some(Some(prog)) = st.programs.get(idx) else {
        return None;
    };
    for target in names {
        for (i, n) in prog.uniform_names.iter().enumerate() {
            if n.as_slice() == *target {
                if let Some(Some(m)) = prog.uniform_mat3.get(i) {
                    return Some(*m);
                }
            }
        }
    }
    None
}

fn transform_xy(st: &GlState, x: f32, y: f32) -> (f32, f32) {
    let mut tx = x;
    let mut ty = y;
    if let Some(m) = find_uniform_mat3(st, st.current_program, &[b"translationMatrix", b"uTranslationMatrix"]) {
        (tx, ty) = mat3_apply(&m, tx, ty);
    }
    if let Some(m) = find_uniform_mat3(st, st.current_program, &[b"projectionMatrix", b"uProjectionMatrix"]) {
        (tx, ty) = mat3_apply(&m, tx, ty);
        return (tx, ty);
    }
    let w = st.viewport_w.max(1) as f32;
    let h = st.viewport_h.max(1) as f32;
    ((2.0 * (tx / w)) - 1.0, 1.0 - (2.0 * (ty / h)))
}

fn pack_vertex(dst: &mut Vec<u8>, x: f32, y: f32, r: u8, g: u8, b: u8) {
    dst.extend_from_slice(&x.to_le_bytes());
    dst.extend_from_slice(&y.to_le_bytes());
    dst.push(r);
    dst.push(g);
    dst.push(b);
    dst.push(0);
}

fn pack_vertex_tex(dst: &mut Vec<u8>, x: f32, y: f32, u: f32, v: f32, r: u8, g: u8, b: u8, a: u8) {
    dst.extend_from_slice(&x.to_le_bytes());
    dst.extend_from_slice(&y.to_le_bytes());
    dst.extend_from_slice(&u.to_le_bytes());
    dst.extend_from_slice(&v.to_le_bytes());
    dst.push(r);
    dst.push(g);
    dst.push(b);
    dst.push(a);
}

fn emit_triangles(st: &mut GlState, indices: &[u32]) {
    let mut pos_attr: Option<(usize, AttribState)> = None;
    let mut preferred_loc: Option<usize> = None;
    if st.current_program != 0 {
        let pidx = st.current_program.saturating_sub(1) as usize;
        if let Some(Some(p)) = st.programs.get(pidx) {
            if let Some((i, _)) = p
                .attrib_names
                .iter()
                .enumerate()
                .find(|(_, n)| n.as_slice() == b"aVertexPosition")
            {
                preferred_loc = Some(i);
            } else if let Some((i, _)) = p
                .attrib_names
                .iter()
                .enumerate()
                .find(|(_, n)| n.as_slice() == b"position")
            {
                preferred_loc = Some(i);
            }
        }
    }
    if let Some(loc) = preferred_loc {
        if loc < MAX_ATTRS {
            let a = st.attribs[loc];
            if a.enabled && a.buffer_id != 0 && a.size >= 2 {
                pos_attr = Some((loc, a));
            }
        }
    }
    if pos_attr.is_none() {
        let a0 = st.attribs[0];
        if a0.enabled && a0.buffer_id != 0 && a0.size >= 2 {
            pos_attr = Some((0, a0));
        }
    }
    if pos_attr.is_none() {
        for i in 0..MAX_ATTRS {
            let a = st.attribs[i];
            if a.enabled && a.buffer_id != 0 && a.size >= 2 && a.type_enum == GL_FLOAT {
                pos_attr = Some((i, a));
                break;
            }
        }
    }
    if pos_attr.is_none() {
        for i in 0..MAX_ATTRS {
            let a = st.attribs[i];
            if a.enabled && a.buffer_id != 0 && a.size >= 2 {
                pos_attr = Some((i, a));
                break;
            }
        }
    }
    let Some((pos_loc, pa)) = pos_attr else {
        return;
    };
    let mut color_attr: Option<AttribState> = None;
    let mut uv_attr: Option<AttribState> = None;
    if st.current_program != 0 {
        let pidx = st.current_program.saturating_sub(1) as usize;
        if let Some(Some(p)) = st.programs.get(pidx) {
            if let Some((i, _)) = p
                .attrib_names
                .iter()
                .enumerate()
                .find(|(_, n)| n.as_slice() == b"aColor" || n.as_slice() == b"color")
            {
                if i < MAX_ATTRS {
                    let a = st.attribs[i];
                    if a.enabled
                        && a.buffer_id != 0
                        && a.size >= 3
                        && (a.type_enum == GL_UNSIGNED_BYTE || a.type_enum == GL_FLOAT)
                    {
                        color_attr = Some(a);
                    }
                }
            }
        }
    }
    if color_attr.is_none() {
        for i in 0..MAX_ATTRS {
            if i == pos_loc {
                continue;
            }
            let a = st.attribs[i];
            if a.enabled
                && a.buffer_id != 0
                && a.size >= 3
                && (a.type_enum == GL_UNSIGNED_BYTE || a.type_enum == GL_FLOAT)
            {
                color_attr = Some(a);
                break;
            }
        }
    }
    if st.current_program != 0 {
        let pidx = st.current_program.saturating_sub(1) as usize;
        if let Some(Some(p)) = st.programs.get(pidx) {
            if let Some((i, _)) = p
                .attrib_names
                .iter()
                .enumerate()
                .find(|(_, n)| n.as_slice() == b"aTextureCoord" || n.as_slice() == b"texCoord")
            {
                if i < MAX_ATTRS {
                    let a = st.attribs[i];
                    if a.enabled && a.buffer_id != 0 && a.size >= 2 && a.type_enum == GL_FLOAT {
                        uv_attr = Some(a);
                    }
                }
            }
        }
    }
    if uv_attr.is_none() {
        for i in 0..MAX_ATTRS {
            if i == pos_loc {
                continue;
            }
            let a = st.attribs[i];
            if a.enabled && a.buffer_id != 0 && a.size >= 2 && a.type_enum == GL_FLOAT {
                if let Some(ca) = color_attr {
                    if ca.buffer_id == a.buffer_id && ca.offset == a.offset {
                        continue;
                    }
                }
                uv_attr = Some(a);
                break;
            }
        }
    }
    let Some(vb) = st
        .buffers
        .get((pa.buffer_id - 1) as usize)
        .and_then(|v| v.as_ref())
    else {
        return;
    };
    if pa.type_enum != GL_FLOAT {
        return;
    }
    let elem = 4usize;
    let stride = if pa.stride <= 0 {
        (pa.size as usize).saturating_mul(elem)
    } else {
        pa.stride as usize
    };
    if stride == 0 {
        return;
    }

    let buffer_rev = st
        .buffer_revs
        .get((pa.buffer_id - 1) as usize)
        .copied()
        .unwrap_or(0);
    let (color_buffer_id, color_buffer_rev, color_stride, color_offset, color_size, color_type_enum) =
        if let Some(ca) = color_attr {
            let cstride = if ca.stride <= 0 {
                match ca.type_enum {
                    GL_UNSIGNED_BYTE => ca.size as usize,
                    GL_FLOAT => (ca.size as usize).saturating_mul(4),
                    _ => 0,
                }
            } else {
                ca.stride as usize
            };
            let crev = st
                .buffer_revs
                .get((ca.buffer_id - 1) as usize)
                .copied()
                .unwrap_or(0);
            (ca.buffer_id, crev, cstride, ca.offset, ca.size, ca.type_enum)
        } else {
            (0, 0, 0, 0, 0, 0)
        };
    let (uv_buffer_id, uv_buffer_rev, uv_stride, uv_offset, uv_size, uv_type_enum) =
        if let Some(ua) = uv_attr {
            let ustride = if ua.stride <= 0 {
                (ua.size as usize).saturating_mul(4)
            } else {
                ua.stride as usize
            };
            let urev = st
                .buffer_revs
                .get((ua.buffer_id - 1) as usize)
                .copied()
                .unwrap_or(0);
            (ua.buffer_id, urev, ustride, ua.offset, ua.size, ua.type_enum)
        } else {
            (0, 0, 0, 0, 0, 0)
        };
    let tex_id = st.current_texture_2d;
    let tex_rev = if tex_id != 0 {
        st.texture_revs
            .get((tex_id - 1) as usize)
            .copied()
            .unwrap_or(0)
    } else {
        0
    };
    let use_tex = uv_attr.is_some()
        && tex_id != 0
        && texture_slot(&st, tex_id)
            .map(|t| t.width > 0 && t.height > 0 && !t.rgba.is_empty())
            .unwrap_or(false);
    let indices_hash = hash_u32_slice(indices);
    let cache_key = VertexDecodeCacheKey {
        buffer_id: pa.buffer_id,
        buffer_rev,
        stride,
        offset: pa.offset,
        size: pa.size,
        type_enum: pa.type_enum,
    };
    let packed_key = PackedVertexCacheKey {
        buffer_id: pa.buffer_id,
        buffer_rev,
        stride,
        offset: pa.offset,
        size: pa.size,
        type_enum: pa.type_enum,
        current_program: st.current_program,
        viewport_w: st.viewport_w,
        viewport_h: st.viewport_h,
        transform_epoch: st.transform_epoch,
        indices_len: indices.len(),
        indices_hash,
        color_buffer_id,
        color_buffer_rev,
        color_stride,
        color_offset,
        color_size,
        color_type_enum,
        uv_buffer_id,
        uv_buffer_rev,
        uv_stride,
        uv_offset,
        uv_size,
        uv_type_enum,
        texture_id: tex_id,
        texture_rev: tex_rev,
    };
    if st.packed_vertex_cache_key == Some(packed_key) && !st.packed_vertex_cache.is_empty() {
        if use_tex {
            cmd_stream::enqueue(cmd_stream::CmdStreamCommand::DrawTrianglesTex {
                tex_id,
                vertices: st.packed_vertex_cache.clone(),
            });
        } else {
            cmd_stream::enqueue(cmd_stream::CmdStreamCommand::DrawTriangles {
                vertices: st.packed_vertex_cache.clone(),
            });
        }
        return;
    }

    let mut decoded_local: Vec<f32> = Vec::new();
    let src_xy: &[f32] = if st.vertex_decode_cache.key == Some(cache_key)
        && st.vertex_decode_cache.indices.as_slice() == indices
    {
        st.vertex_decode_cache.xy.as_slice()
    } else {
        decoded_local.reserve(indices.len().saturating_mul(2));
        for idx in indices {
            let off = pa.offset.saturating_add((*idx as usize).saturating_mul(stride));
            let Some(px) = vb.get(off..off + 4) else {
                continue;
            };
            let Some(py) = vb.get(off + 4..off + 8) else {
                continue;
            };
            let x = f32::from_le_bytes([px[0], px[1], px[2], px[3]]);
            let y = f32::from_le_bytes([py[0], py[1], py[2], py[3]]);
            decoded_local.push(x);
            decoded_local.push(y);
        }
        decoded_local.as_slice()
    };

    let mut out = Vec::with_capacity((src_xy.len() / 2).saturating_mul(if use_tex { 20 } else { 12 }));
    let color_buf = color_attr.and_then(|ca| {
        st.buffers
            .get((ca.buffer_id - 1) as usize)
            .and_then(|v| v.as_ref())
    });
    let uv_buf = uv_attr.and_then(|ua| {
        st.buffers
            .get((ua.buffer_id - 1) as usize)
            .and_then(|v| v.as_ref())
    });
    let mut i = 0usize;
    while i + 1 < src_xy.len() {
        let x = src_xy[i];
        let y = src_xy[i + 1];
        let (nx, ny) = transform_xy(st, x, y);
        let mut r: u8 = 255;
        let mut g: u8 = 255;
        let mut b: u8 = 255;
        let mut a: u8 = 255;
        let mut u: f32 = 0.0;
        let mut v: f32 = 0.0;
        if let (Some(ca), Some(cb)) = (color_attr, color_buf) {
            let idx_i = i / 2;
            let cstride = if ca.stride <= 0 {
                match ca.type_enum {
                    GL_UNSIGNED_BYTE => ca.size as usize,
                    GL_FLOAT => (ca.size as usize).saturating_mul(4),
                    _ => 0,
                }
            } else {
                ca.stride as usize
            };
            let coff = ca
                .offset
                .saturating_add((indices[idx_i] as usize).saturating_mul(cstride));
            match ca.type_enum {
                GL_UNSIGNED_BYTE => {
                    if let Some(bytes) = cb.get(coff..coff.saturating_add(4)) {
                        r = bytes[0];
                        g = bytes[1];
                        b = bytes[2];
                        if ca.size >= 4 && bytes.len() > 3 {
                            a = bytes[3];
                        }
                    }
                }
                GL_FLOAT => {
                    if let Some(px) = cb.get(coff..coff + 4) {
                        let v = f32::from_le_bytes([px[0], px[1], px[2], px[3]]).clamp(0.0, 1.0);
                        r = (v * 255.0) as u8;
                    }
                    if let Some(py) = cb.get(coff + 4..coff + 8) {
                        let v = f32::from_le_bytes([py[0], py[1], py[2], py[3]]).clamp(0.0, 1.0);
                        g = (v * 255.0) as u8;
                    }
                    if let Some(pz) = cb.get(coff + 8..coff + 12) {
                        let v = f32::from_le_bytes([pz[0], pz[1], pz[2], pz[3]]).clamp(0.0, 1.0);
                        b = (v * 255.0) as u8;
                    }
                    if ca.size >= 4 {
                        if let Some(pw) = cb.get(coff + 12..coff + 16) {
                            let v = f32::from_le_bytes([pw[0], pw[1], pw[2], pw[3]]).clamp(0.0, 1.0);
                            a = (v * 255.0) as u8;
                        }
                    }
                }
                _ => {}
            }
        }
        if use_tex {
            if let (Some(ua), Some(ub)) = (uv_attr, uv_buf) {
                let idx_i = i / 2;
                let ustride = if ua.stride <= 0 {
                    (ua.size as usize).saturating_mul(4)
                } else {
                    ua.stride as usize
                };
                let uoff = ua
                    .offset
                    .saturating_add((indices[idx_i] as usize).saturating_mul(ustride));
                if let Some(px) = ub.get(uoff..uoff + 4) {
                    u = f32::from_le_bytes([px[0], px[1], px[2], px[3]]);
                }
                if let Some(py) = ub.get(uoff + 4..uoff + 8) {
                    v = f32::from_le_bytes([py[0], py[1], py[2], py[3]]);
                }
            }
            pack_vertex_tex(&mut out, nx, ny, u, v, r, g, b, a);
        } else {
            pack_vertex(&mut out, nx, ny, r, g, b);
        }
        i += 2;
    }

    if st.vertex_decode_cache.key != Some(cache_key) || st.vertex_decode_cache.indices.as_slice() != indices {
        st.vertex_decode_cache.key = Some(cache_key);
        st.vertex_decode_cache.indices.clear();
        st.vertex_decode_cache.indices.extend_from_slice(indices);
        st.vertex_decode_cache.xy = decoded_local;
    }

    if !out.is_empty() {
        st.packed_vertex_cache_key = Some(packed_key);
        st.packed_vertex_cache.clear();
        st.packed_vertex_cache.extend_from_slice(out.as_slice());
        if use_tex {
            cmd_stream::enqueue(cmd_stream::CmdStreamCommand::DrawTrianglesTex {
                tex_id,
                vertices: out,
            });
        } else {
            cmd_stream::enqueue(cmd_stream::CmdStreamCommand::DrawTriangles { vertices: out });
        }
    }
}

fn begin_frame_if_needed(st: &mut GlState) {
    if st.frame_open {
        return;
    }
    if st.viewport_dirty {
        cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetViewport {
            w: st.viewport_w.max(1),
            h: st.viewport_h.max(1),
        });
        st.viewport_dirty = false;
    }
    if st.clear_dirty {
        cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetClearColor {
            clear_rgb: st.clear_rgb,
        });
        st.clear_dirty = false;
    }
    if st.blend_dirty {
        cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetBlendEnabled {
            enabled: st.blend_enabled,
        });
        st.blend_dirty = false;
    }
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::BeginFrame);
    st.frame_open = true;
}

unsafe extern "C" fn gl_create_buffer(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let mut st = GL_STATE.lock();
    let id = st.alloc_handle();
    let idx = (id - 1) as usize;
    if idx >= st.buffers.len() {
        st.buffers.resize_with(idx + 1, || None);
    }
    if idx >= st.buffer_revs.len() {
        st.buffer_revs.resize(idx + 1, 0);
    }
    st.buffers[idx] = Some(Vec::new());
    st.buffer_revs[idx] = 1;
    js_new_handle_obj(ctx, id)
}

unsafe extern "C" fn gl_create_texture(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let mut st = GL_STATE.lock();
    let id = st.alloc_handle();
    let idx = (id - 1) as usize;
    if idx >= st.textures.len() {
        st.textures.resize_with(idx + 1, || None);
    }
    if idx >= st.texture_revs.len() {
        st.texture_revs.resize(idx + 1, 0);
    }
    st.textures[idx] = Some(TextureState {
        width: 0,
        height: 0,
        rgba: Vec::new(),
        rev: 1,
    });
    st.texture_revs[idx] = 1;
    js_new_handle_obj(ctx, id)
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
    let Some(target) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    let id = js_get_handle_id(ctx, args[1]).unwrap_or(0);
    let mut st = GL_STATE.lock();
    if target == GL_TEXTURE_2D {
        st.current_texture_2d = id;
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_pixel_storei(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(pname) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    let Some(value) = js_get_f64(ctx, args[1]).map(|x| x as i32) else {
        return qjs::JSValue::undefined();
    };
    if pname == GL_UNPACK_ALIGNMENT {
        let mut st = GL_STATE.lock();
        st.unpack_alignment = value;
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_tex_image_2d(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 6 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(target) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    if target != GL_TEXTURE_2D {
        return qjs::JSValue::undefined();
    }

    let (width, height, format, ty, data_ptr, data_len) = if argc >= 9 {
        let Some(width) = js_get_f64(ctx, args[3]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(height) = js_get_f64(ctx, args[4]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(format) = js_get_f64(ctx, args[6]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(ty) = js_get_f64(ctx, args[7]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let mut ptr = core::ptr::null();
        let mut len = 0usize;
        if let Some((p, l)) = js_get_arraybuffer_view(ctx, args[8]) {
            ptr = p;
            len = l;
        }
        (width, height, format, ty, ptr, len)
    } else {
        let Some(format) = js_get_f64(ctx, args[3]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(ty) = js_get_f64(ctx, args[4]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let source = args[5];
        let Some(width) = js_get_obj_u32(ctx, source, b"width\0".as_ptr() as *const c_char) else {
            return qjs::JSValue::undefined();
        };
        let Some(height) = js_get_obj_u32(ctx, source, b"height\0".as_ptr() as *const c_char) else {
            return qjs::JSValue::undefined();
        };
        let pixels = js_get_obj_arraybuffer_view(ctx, source, b"pixels\0".as_ptr() as *const c_char)
            .or_else(|| js_get_obj_arraybuffer_view(ctx, source, b"data\0".as_ptr() as *const c_char));
        let (ptr, len) = pixels.unwrap_or((core::ptr::null(), 0));
        (width, height, format, ty, ptr, len)
    };

    if format != GL_RGBA || ty != GL_UNSIGNED_BYTE || width == 0 || height == 0 {
        return qjs::JSValue::undefined();
    }

    let mut st = GL_STATE.lock();
    let tex_id = st.current_texture_2d;
    let expected = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    let mut rgba = vec![0u8; expected];
    if !data_ptr.is_null() && data_len > 0 {
        let src = core::slice::from_raw_parts(data_ptr, data_len.min(expected));
        rgba[..src.len()].copy_from_slice(src);
    }

    let upload_rgba = rgba.clone();
    {
        let Some(tex) = texture_slot_mut(&mut st, tex_id) else {
            return qjs::JSValue::undefined();
        };
        tex.width = width;
        tex.height = height;
        tex.rgba = rgba;
        tex.rev = tex.rev.wrapping_add(1).max(1);
    }

    let idx = (tex_id - 1) as usize;
    if let Some(rev) = st.texture_revs.get_mut(idx) {
        *rev = rev.wrapping_add(1);
    }

    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::UploadTexture {
        tex_id,
        width,
        height,
        rgba: upload_rgba,
    });
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_tex_sub_image_2d(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 7 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(target) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    let (xoff, yoff, width, height, format, ty, ptr, len) = if argc >= 9 {
        let Some(xoff) = js_get_f64(ctx, args[2]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(yoff) = js_get_f64(ctx, args[3]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(width) = js_get_f64(ctx, args[4]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(height) = js_get_f64(ctx, args[5]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(format) = js_get_f64(ctx, args[6]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(ty) = js_get_f64(ctx, args[7]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[8]) else {
            return qjs::JSValue::undefined();
        };
        (xoff, yoff, width, height, format, ty, ptr, len)
    } else {
        let Some(xoff) = js_get_f64(ctx, args[2]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(yoff) = js_get_f64(ctx, args[3]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(format) = js_get_f64(ctx, args[4]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let Some(ty) = js_get_f64(ctx, args[5]).map(|x| x.max(0.0) as u32) else {
            return qjs::JSValue::undefined();
        };
        let source = args[6];
        let Some(width) = js_get_obj_u32(ctx, source, b"width\0".as_ptr() as *const c_char) else {
            return qjs::JSValue::undefined();
        };
        let Some(height) = js_get_obj_u32(ctx, source, b"height\0".as_ptr() as *const c_char) else {
            return qjs::JSValue::undefined();
        };
        let pixels = js_get_obj_arraybuffer_view(ctx, source, b"pixels\0".as_ptr() as *const c_char)
            .or_else(|| js_get_obj_arraybuffer_view(ctx, source, b"data\0".as_ptr() as *const c_char));
        let (ptr, len) = pixels.unwrap_or((core::ptr::null(), 0));
        (xoff, yoff, width, height, format, ty, ptr, len)
    };

    if target != GL_TEXTURE_2D || format != GL_RGBA || ty != GL_UNSIGNED_BYTE {
        return qjs::JSValue::undefined();
    }

    let mut st = GL_STATE.lock();
    let tex_id = st.current_texture_2d;
    let expected = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    if ptr.is_null() || len == 0 {
        return qjs::JSValue::undefined();
    }
    let src = core::slice::from_raw_parts(ptr, len.min(expected));

    let (tex_w, tex_h, upload_rgba) = {
        let Some(tex) = texture_slot_mut(&mut st, tex_id) else {
            return qjs::JSValue::undefined();
        };
        if tex.width == 0 || tex.height == 0 {
            return qjs::JSValue::undefined();
        }
        let tex_w = tex.width as usize;
        for row in 0..height as usize {
            let dst_y = yoff as usize + row;
            if dst_y >= tex.height as usize {
                break;
            }
            let dst_x = xoff as usize;
            if dst_x >= tex_w {
                break;
            }
            let row_bytes = (width as usize).saturating_mul(4);
            let src_off = row.saturating_mul(row_bytes);
            let dst_off = (dst_y * tex_w + dst_x) * 4;
            let copy_len = row_bytes.min(tex.rgba.len().saturating_sub(dst_off));
            if src_off + copy_len <= src.len() {
                tex.rgba[dst_off..dst_off + copy_len]
                    .copy_from_slice(&src[src_off..src_off + copy_len]);
            }
        }
        tex.rev = tex.rev.wrapping_add(1).max(1);
        (tex.width, tex.height, tex.rgba.clone())
    };

    let idx = (tex_id - 1) as usize;
    if let Some(rev) = st.texture_revs.get_mut(idx) {
        *rev = rev.wrapping_add(1);
    }

    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::UploadTexture {
        tex_id,
        width: tex_w,
        height: tex_h,
        rgba: upload_rgba,
    });
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
    let Some(target) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    let id = js_get_handle_id(ctx, args[1]).unwrap_or(0);
    let mut st = GL_STATE.lock();
    if target == GL_ARRAY_BUFFER {
        st.current_array_buffer = id;
    } else if target == GL_ELEMENT_ARRAY_BUFFER {
        st.current_element_array_buffer = id;
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_buffer_data(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(target) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    let mut st = GL_STATE.lock();
    let buf_id = if target == GL_ARRAY_BUFFER {
        st.current_array_buffer
    } else if target == GL_ELEMENT_ARRAY_BUFFER {
        st.current_element_array_buffer
    } else {
        0
    };
    let Some(dst) = buffer_slot_mut(&mut st, buf_id) else {
        return qjs::JSValue::undefined();
    };
    if let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[1]) {
        dst.clear();
        dst.extend_from_slice(core::slice::from_raw_parts(ptr, len));
    } else if let Some(sz) = js_get_f64(ctx, args[1]).map(|x| x.max(0.0) as usize) {
        dst.clear();
        dst.resize(sz, 0);
    }
    if buf_id != 0 {
        let idx = (buf_id - 1) as usize;
        if let Some(rev) = st.buffer_revs.get_mut(idx) {
            *rev = rev.wrapping_add(1);
        }
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
    let Some(target) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    let Some(offset) = js_get_f64(ctx, args[1]).map(|x| x.max(0.0) as usize) else {
        return qjs::JSValue::undefined();
    };
    let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[2]) else {
        return qjs::JSValue::undefined();
    };
    let src = core::slice::from_raw_parts(ptr, len);
    let mut st = GL_STATE.lock();
    let buf_id = if target == GL_ARRAY_BUFFER {
        st.current_array_buffer
    } else if target == GL_ELEMENT_ARRAY_BUFFER {
        st.current_element_array_buffer
    } else {
        0
    };
    let Some(dst) = buffer_slot_mut(&mut st, buf_id) else {
        return qjs::JSValue::undefined();
    };
    let need = offset.saturating_add(src.len());
    if need > dst.len() {
        dst.resize(need, 0);
    }
    dst[offset..offset + src.len()].copy_from_slice(src);
    if buf_id != 0 {
        let idx = (buf_id - 1) as usize;
        if let Some(rev) = st.buffer_revs.get_mut(idx) {
            *rev = rev.wrapping_add(1);
        }
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_create_program(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let mut st = GL_STATE.lock();
    let id = st.alloc_handle();
    let idx = (id - 1) as usize;
    if idx >= st.programs.len() {
        st.programs.resize_with(idx + 1, || None);
    }
    st.programs[idx] = Some(ProgramState::new());
    js_new_handle_obj(ctx, id)
}

unsafe extern "C" fn gl_create_shader(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let mut shader_type = GL_FRAGMENT_SHADER;
    if !argv.is_null() && argc >= 1 {
        let args = core::slice::from_raw_parts(argv, argc as usize);
        shader_type = js_get_f64(ctx, args[0]).unwrap_or(GL_FRAGMENT_SHADER as f64).max(0.0) as u32;
    }
    let mut st = GL_STATE.lock();
    let id = st.alloc_handle();
    let idx = (id - 1) as usize;
    if idx >= st.shaders.len() {
        st.shaders.resize_with(idx + 1, || None);
    }
    st.shaders[idx] = Some(ShaderState {
        shader_type,
        source: Vec::new(),
        compiled: false,
    });
    js_new_handle_obj(ctx, id)
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
    let shader_id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[1], 0);
    if cstr.is_null() {
        return qjs::JSValue::undefined();
    }
    let src = core::slice::from_raw_parts(cstr as *const u8, len).to_vec();
    qjs::JS_FreeCString(ctx, cstr);
    let mut st = GL_STATE.lock();
    if let Some(sh) = shader_slot_mut(&mut st, shader_id) {
        sh.source = src;
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_compile_shader(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let shader_id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let mut st = GL_STATE.lock();
    if let Some(sh) = shader_slot_mut(&mut st, shader_id) {
        sh.compiled = true;
    }
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
    let prog_id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let shader_id = js_get_handle_id(ctx, args[1]).unwrap_or(0);
    let mut st = GL_STATE.lock();
    let shader_type = shader_slot(&st, shader_id).map(|s| s.shader_type).unwrap_or(0);
    if let Some(p) = program_slot_mut(&mut st, prog_id) {
        if shader_type == GL_VERTEX_SHADER {
            p.attached_vertex_shader = shader_id;
        } else if shader_type == GL_FRAGMENT_SHADER {
            p.attached_fragment_shader = shader_id;
        }
    }
    qjs::JSValue::undefined()
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
    let prog_id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let loc = js_get_f64(ctx, args[1]).unwrap_or(0.0).max(0.0) as usize;
    let name_c = qjs::js_to_cstring(ctx, args[2]);
    if name_c.is_null() {
        return qjs::JSValue::undefined();
    }
    let name = CStr::from_ptr(name_c).to_bytes().to_vec();
    qjs::JS_FreeCString(ctx, name_c);
    let mut st = GL_STATE.lock();
    if let Some(p) = program_slot_mut(&mut st, prog_id) {
        if p.attrib_names.len() <= loc {
            p.attrib_names.resize(loc + 1, Vec::new());
        }
        p.attrib_names[loc] = name;
    }
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
    let prog_id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let mut st = GL_STATE.lock();
    let (vs_id, fs_id) = st
        .programs
        .get(prog_id.saturating_sub(1) as usize)
        .and_then(|p| p.as_ref())
        .map(|p| (p.attached_vertex_shader, p.attached_fragment_shader))
        .unwrap_or((0, 0));
    let vs_src = shader_slot(&st, vs_id).map(|s| s.source.clone()).unwrap_or_default();
    let fs_src = shader_slot(&st, fs_id).map(|s| s.source.clone()).unwrap_or_default();
    if let Some(p) = program_slot_mut(&mut st, prog_id) {
        p.active_attribs.clear();
        p.active_uniforms.clear();
        scan_glsl_decl(vs_src.as_slice(), b"attribute ", &mut p.active_attribs);
        scan_glsl_decl(vs_src.as_slice(), b"in ", &mut p.active_attribs);
        scan_glsl_decl(vs_src.as_slice(), b"uniform ", &mut p.active_uniforms);
        scan_glsl_decl(fs_src.as_slice(), b"uniform ", &mut p.active_uniforms);
        if p.attrib_names.is_empty() {
            for (n, _, _) in p.active_attribs.iter() {
                p.attrib_names.push(n.clone());
            }
        }
        if p.uniform_names.is_empty() {
            for (n, _, _) in p.active_uniforms.iter() {
                p.uniform_names.push(n.clone());
                p.uniform_mat3.push(None);
            }
        }
        p.linked = true;
    }
    qjs::JSValue::undefined()
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
    let prog_id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let idx = js_get_f64(ctx, args[1]).unwrap_or(0.0).max(0.0) as usize;
    let st = GL_STATE.lock();
    let Some(Some(p)) = st.programs.get(prog_id.saturating_sub(1) as usize) else {
        return js_null();
    };
    let Some((name, type_enum, size)) = p.active_attribs.get(idx) else {
        return js_null();
    };
    let o = qjs::JS_NewObject(ctx);
    if o.is_exception() {
        return o;
    }
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        o,
        b"name\0".as_ptr() as *const c_char,
        qjs::JS_NewStringLen(ctx, name.as_ptr() as *const c_char, name.len()),
    );
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"type\0".as_ptr() as *const c_char, js_int32(*type_enum as i32));
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"size\0".as_ptr() as *const c_char, js_int32(*size));
    o
}

unsafe extern "C" fn gl_get_active_uniform(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let prog_id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let idx = js_get_f64(ctx, args[1]).unwrap_or(0.0).max(0.0) as usize;
    let st = GL_STATE.lock();
    let Some(Some(p)) = st.programs.get(prog_id.saturating_sub(1) as usize) else {
        return js_null();
    };
    let Some((name, type_enum, size)) = p.active_uniforms.get(idx) else {
        return js_null();
    };
    let o = qjs::JS_NewObject(ctx);
    if o.is_exception() {
        return o;
    }
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        o,
        b"name\0".as_ptr() as *const c_char,
        qjs::JS_NewStringLen(ctx, name.as_ptr() as *const c_char, name.len()),
    );
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"type\0".as_ptr() as *const c_char, js_int32(*type_enum as i32));
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"size\0".as_ptr() as *const c_char, js_int32(*size));
    o
}

unsafe extern "C" fn gl_use_program(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    GL_STATE.lock().current_program = id;
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_get_attrib_location(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return js_int32(0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let prog_id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let name_c = qjs::js_to_cstring(ctx, args[1]);
    if name_c.is_null() {
        return js_int32(0);
    }
    let name = CStr::from_ptr(name_c).to_bytes().to_vec();
    qjs::JS_FreeCString(ctx, name_c);
    let mut st = GL_STATE.lock();
    let Some(p) = program_slot_mut(&mut st, prog_id) else {
        return js_int32(0);
    };
    if let Some((idx, _)) = p.attrib_names.iter().enumerate().find(|(_, n)| **n == name) {
        return js_int32(idx as i32);
    }
    let idx = p.attrib_names.len().min(MAX_ATTRS.saturating_sub(1));
    if idx >= p.attrib_names.len() {
        p.attrib_names.push(name);
    }
    js_int32(idx as i32)
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
    let Some(loc) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as usize) else {
        return qjs::JSValue::undefined();
    };
    let mut st = GL_STATE.lock();
    if loc < MAX_ATTRS {
        st.attribs[loc].enabled = true;
    }
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
    let Some(loc) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as usize) else {
        return qjs::JSValue::undefined();
    };
    let mut st = GL_STATE.lock();
    if loc < MAX_ATTRS {
        st.attribs[loc].enabled = false;
    }
    qjs::JSValue::undefined()
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
    let Some(loc) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as usize) else {
        return qjs::JSValue::undefined();
    };
    if loc >= MAX_ATTRS {
        return qjs::JSValue::undefined();
    }
    let Some(size) = js_get_f64(ctx, args[1]).map(|x| x as i32) else {
        return qjs::JSValue::undefined();
    };
    let Some(type_enum) = js_get_f64(ctx, args[2]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    let normalized = js_get_f64(ctx, args[3]).unwrap_or(0.0) != 0.0;
    let Some(stride) = js_get_f64(ctx, args[4]).map(|x| x.max(0.0) as i32) else {
        return qjs::JSValue::undefined();
    };
    let Some(offset) = js_get_f64(ctx, args[5]).map(|x| x.max(0.0) as usize) else {
        return qjs::JSValue::undefined();
    };
    let mut st = GL_STATE.lock();
    st.attribs[loc] = AttribState {
        enabled: st.attribs[loc].enabled,
        size,
        type_enum,
        normalized,
        stride,
        offset,
        buffer_id: st.current_array_buffer,
    };
    qjs::JSValue::undefined()
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
    let prog_id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let name_c = qjs::js_to_cstring(ctx, args[1]);
    if name_c.is_null() {
        return js_null();
    }
    let raw_name = CStr::from_ptr(name_c).to_bytes().to_vec();
    qjs::JS_FreeCString(ctx, name_c);
    let name = sanitize_glsl_name(raw_name.as_slice());
    let mut st = GL_STATE.lock();
    let Some(p) = program_slot_mut(&mut st, prog_id) else {
        return js_null();
    };
    let idx = if let Some((i, _)) = p.uniform_names.iter().enumerate().find(|(_, n)| **n == name) {
        i
    } else {
        p.uniform_names.push(name);
        p.uniform_mat3.push(None);
        p.uniform_names.len() - 1
    };
    let o = qjs::JS_NewObject(ctx);
    if o.is_exception() {
        return o;
    }
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        o,
        b"__u\0".as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, idx as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        o,
        b"__p\0".as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, prog_id as f64),
    );
    o
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
    let loc_obj = args[0];
    let Some(loc) = uniform_loc_index(ctx, loc_obj) else {
        return qjs::JSValue::undefined();
    };
    let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[2]) else {
        return qjs::JSValue::undefined();
    };
    if len < 36 {
        return qjs::JSValue::undefined();
    }
    let src = core::slice::from_raw_parts(ptr, 36);
    let mut m = [0.0f32; 9];
    for i in 0..9 {
        let b = &src[i * 4..i * 4 + 4];
        m[i] = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    }
    let mut st = GL_STATE.lock();
    let prog_id = {
        let p = uniform_loc_program(ctx, loc_obj);
        if p != 0 { p } else { st.current_program }
    };
    let Some(p) = program_slot_mut(&mut st, prog_id) else {
        return qjs::JSValue::undefined();
    };
    if loc < p.uniform_mat3.len() {
        let changed = p.uniform_mat3[loc].map(|old| old != m).unwrap_or(true);
        p.uniform_mat3[loc] = Some(m);
        if changed {
            st.transform_epoch = st.transform_epoch.wrapping_add(1);
        }
    }
    qjs::JSValue::undefined()
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
    let r = (js_get_f64(ctx, args[0]).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0) as u32;
    let g = (js_get_f64(ctx, args[1]).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0) as u32;
    let b = (js_get_f64(ctx, args[2]).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0) as u32;
    let mut st = GL_STATE.lock();
    let rgb = (r << 16) | (g << 8) | b;
    if st.clear_rgb != rgb {
        st.clear_rgb = rgb;
        st.clear_dirty = true;
    }
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
    let Some(mask) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    if (mask & GL_COLOR_BUFFER_BIT) != 0 {
        let mut st = GL_STATE.lock();
        // Treat COLOR clear as a frame boundary:
        // Pixi with clearBeforeRender=true issues clear once per render().
        // Closing an open frame here guarantees presents even if GL flush/finish
        // is not called reliably by the JS side.
        if st.frame_open {
            cmd_stream::enqueue(cmd_stream::CmdStreamCommand::EndFrame);
            st.frame_open = false;
        }
        begin_frame_if_needed(&mut st);
    }
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
    let w = js_get_f64(ctx, args[2]).unwrap_or(0.0).max(0.0) as i32;
    let h = js_get_f64(ctx, args[3]).unwrap_or(0.0).max(0.0) as i32;
    let mut st = GL_STATE.lock();
    let nw = w.max(1);
    let nh = h.max(1);
    if st.viewport_w != nw || st.viewport_h != nh {
        st.viewport_w = nw;
        st.viewport_h = nh;
        st.transform_epoch = st.transform_epoch.wrapping_add(1);
        st.viewport_dirty = true;
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_flush_frame(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let mut st = GL_STATE.lock();
    if st.frame_open {
        cmd_stream::enqueue(cmd_stream::CmdStreamCommand::EndFrame);
        st.frame_open = false;
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_enable_disable(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    enabled: bool,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(cap) = js_get_f64(ctx, args[0]).map(|x| x.max(0.0) as u32) else {
        return qjs::JSValue::undefined();
    };
    if cap == GL_BLEND {
        let mut st = GL_STATE.lock();
        if st.blend_enabled != enabled {
            st.blend_enabled = enabled;
            st.blend_dirty = true;
        }
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_uniform_1f(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_2f(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_3f(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_4f(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_1i(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_2i(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_3i(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_4i(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_1fv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_2fv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_3fv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_4fv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_1iv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_2iv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_3iv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_4iv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_1ui(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_2ui(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_3ui(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_4ui(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_1uiv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_2uiv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_3uiv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_4uiv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_matrix2fv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_uniform_matrix4fv(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_uniform_store_noop(ctx, this_val, argc, argv)
}

unsafe extern "C" fn gl_enable(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_enable_disable(ctx, this_val, argc, argv, true)
}

unsafe extern "C" fn gl_disable(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    gl_enable_disable(ctx, this_val, argc, argv, false)
}

unsafe extern "C" fn gl_draw_elements(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 4 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mode = js_get_f64(ctx, args[0]).unwrap_or(0.0).max(0.0) as u32;
    let count = js_get_f64(ctx, args[1]).unwrap_or(0.0).max(0.0) as usize;
    let index_type = js_get_f64(ctx, args[2]).unwrap_or(0.0).max(0.0) as u32;
    let index_offset = js_get_f64(ctx, args[3]).unwrap_or(0.0).max(0.0) as usize;
    if count < 3 {
        return qjs::JSValue::undefined();
    }
    let mut st = GL_STATE.lock();
    let elem_id = st.current_element_array_buffer;
    let idx_src = {
        let Some(Some(ib)) = st.buffers.get(elem_id.saturating_sub(1) as usize) else {
            return qjs::JSValue::undefined();
        };
        let mut idx_src = Vec::with_capacity(count);
        for i in 0..count {
            let Some(v) = read_index(ib, index_type, index_offset, i) else {
                break;
            };
            idx_src.push(v);
        }
        idx_src
    };
    if idx_src.len() < 3 {
        return qjs::JSValue::undefined();
    }
    let elem_rev = st
        .buffer_revs
        .get(elem_id.saturating_sub(1) as usize)
        .copied()
        .unwrap_or(0);
    let cache_key = IndexExpandCacheKey {
        elem_buffer_id: elem_id,
        elem_buffer_rev: elem_rev,
        mode,
        count: idx_src.len(),
        index_type,
        index_offset,
    };

    if st.index_expand_cache.key == Some(cache_key) {
        let tri = core::mem::take(&mut st.index_expand_cache.tri);
        if tri.len() >= 3 {
            begin_frame_if_needed(&mut st);
            emit_triangles(&mut st, tri.as_slice());
        }
        st.index_expand_cache.tri = tri;
        return qjs::JSValue::undefined();
    }

    let mut tri = Vec::new();
    match mode {
        GL_TRIANGLES => {
            let tris = idx_src.len() / 3;
            tri.reserve(tris * 3);
            for t in 0..tris {
                let b = t * 3;
                tri.push(idx_src[b]);
                tri.push(idx_src[b + 1]);
                tri.push(idx_src[b + 2]);
            }
        }
        GL_TRIANGLE_STRIP => {
            tri.reserve((idx_src.len() - 2) * 3);
            for i in 0..(idx_src.len() - 2) {
                if (i & 1) == 0 {
                    tri.push(idx_src[i]);
                    tri.push(idx_src[i + 1]);
                    tri.push(idx_src[i + 2]);
                } else {
                    tri.push(idx_src[i + 1]);
                    tri.push(idx_src[i]);
                    tri.push(idx_src[i + 2]);
                }
            }
        }
        GL_TRIANGLE_FAN => {
            tri.reserve((idx_src.len() - 2) * 3);
            let base = idx_src[0];
            for i in 1..(idx_src.len() - 1) {
                tri.push(base);
                tri.push(idx_src[i]);
                tri.push(idx_src[i + 1]);
            }
        }
        _ => {}
    }
    if tri.len() >= 3 {
        begin_frame_if_needed(&mut st);
        emit_triangles(&mut st, tri.as_slice());
        st.index_expand_cache.key = Some(cache_key);
        st.index_expand_cache.tri = tri;
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
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mode = js_get_f64(ctx, args[0]).unwrap_or(0.0).max(0.0) as u32;
    let first = js_get_f64(ctx, args[1]).unwrap_or(0.0).max(0.0) as u32;
    let count = js_get_f64(ctx, args[2]).unwrap_or(0.0).max(0.0) as u32;
    if count < 3 || mode != GL_TRIANGLES {
        return qjs::JSValue::undefined();
    }
    let mut idx = Vec::with_capacity(count as usize);
    for i in 0..count {
        idx.push(first + i);
    }
    let mut st = GL_STATE.lock();
    begin_frame_if_needed(&mut st);
    emit_triangles(&mut st, idx.as_slice());
    qjs::JSValue::undefined()
}

unsafe extern "C" fn gl_get_shader_program_parameter(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return js_bool(true);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let id = js_get_handle_id(ctx, args[0]).unwrap_or(0);
    let pname = js_get_f64(ctx, args[1]).unwrap_or(0.0).max(0.0) as u32;
    let st = GL_STATE.lock();
    if pname == GL_COMPILE_STATUS {
        let ok = shader_slot(&st, id).map(|s| s.compiled).unwrap_or(true);
        return js_bool(ok);
    }
    if pname == GL_LINK_STATUS {
        let ok = st
            .programs
            .get(id.saturating_sub(1) as usize)
            .and_then(|p| p.as_ref())
            .map(|p| p.linked)
            .unwrap_or(true);
        return js_bool(ok);
    }
    if pname == GL_ACTIVE_ATTRIBUTES {
        let n = st
            .programs
            .get(id.saturating_sub(1) as usize)
            .and_then(|p| p.as_ref())
            .map(|p| p.active_attribs.len() as i32)
            .unwrap_or(0);
        return js_int32(n);
    }
    if pname == GL_ACTIVE_UNIFORMS {
        let n = st
            .programs
            .get(id.saturating_sub(1) as usize)
            .and_then(|p| p.as_ref())
            .map(|p| p.active_uniforms.len() as i32)
            .unwrap_or(0);
        return js_int32(n);
    }
    js_int32(1)
}

unsafe extern "C" fn gl_get_parameter(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let pname = js_get_f64(ctx, args[0]).unwrap_or(0.0).max(0.0) as u32;
    match pname {
        GL_VERSION => qjs::JS_NewStringLen(ctx, b"WebGL 1.0 TRUEOS\0".as_ptr() as *const c_char, 16),
        GL_RENDERER => qjs::JS_NewStringLen(ctx, b"TRUEOS CmdStream\0".as_ptr() as *const c_char, 16),
        GL_VENDOR => qjs::JS_NewStringLen(ctx, b"TRUEOS\0".as_ptr() as *const c_char, 6),
        GL_MAX_VERTEX_ATTRIBS => js_int32(MAX_ATTRS as i32),
        GL_MAX_TEXTURE_IMAGE_UNITS => js_int32(8),
        GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS => js_int32(8),
        GL_STENCIL_BITS => js_int32(8),
        _ => js_int32(0),
    }
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
    let name_c = qjs::js_to_cstring(ctx, args[0]);
    if name_c.is_null() {
        return js_null();
    }
    let name = CStr::from_ptr(name_c).to_bytes();
    qjs::JS_FreeCString(ctx, name_c);
    if name.eq_ignore_ascii_case(b"OES_vertex_array_object")
        || name.eq_ignore_ascii_case(b"OES_element_index_uint")
        || name.eq_ignore_ascii_case(b"ANGLE_instanced_arrays")
        || name.eq_ignore_ascii_case(b"WEBGL_draw_buffers")
    {
        return qjs::JS_NewObject(ctx);
    }
    if name.eq_ignore_ascii_case(b"EXT_texture_filter_anisotropic")
        || name.eq_ignore_ascii_case(b"MOZ_EXT_texture_filter_anisotropic")
        || name.eq_ignore_ascii_case(b"WEBKIT_EXT_texture_filter_anisotropic")
    {
        let ext = qjs::JS_NewObject(ctx);
        if ext.is_exception() {
            return ext;
        }
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            ext,
            b"TEXTURE_MAX_ANISOTROPY_EXT\0".as_ptr() as *const c_char,
            js_int32(0x84FE),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            ext,
            b"MAX_TEXTURE_MAX_ANISOTROPY_EXT\0".as_ptr() as *const c_char,
            js_int32(0x84FF),
        );
        return ext;
    }
    js_null()
}

unsafe extern "C" fn gl_noop(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs::JSValue::undefined()
}

unsafe extern "C" fn canvas2d_noop(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs::JSValue::undefined()
}

unsafe extern "C" fn canvas2d_measure_text(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let m = qjs::JS_NewObject(ctx);
    if m.is_exception() {
        return m;
    }
    let _ = qjs::JS_SetPropertyStr(ctx, m, b"width\0".as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, 0.0));
    m
}

unsafe fn make_canvas_2d_context(ctx: *mut qjs::JSContext, canvas: qjs::JSValueConst) -> qjs::JSValue {
    let c2d = qjs::JS_NewObject(ctx);
    if c2d.is_exception() {
        return c2d;
    }
    macro_rules! c2d_fn {
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
            let _ = qjs::JS_SetPropertyStr(ctx, c2d, k.as_ptr() as *const c_char, f);
        }};
    }
    c2d_fn!("fillRect", canvas2d_noop, 4);
    c2d_fn!("clearRect", canvas2d_noop, 4);
    c2d_fn!("drawImage", canvas2d_noop, 9);
    c2d_fn!("save", canvas2d_noop, 0);
    c2d_fn!("restore", canvas2d_noop, 0);
    c2d_fn!("translate", canvas2d_noop, 2);
    c2d_fn!("rotate", canvas2d_noop, 1);
    c2d_fn!("scale", canvas2d_noop, 2);
    c2d_fn!("setTransform", canvas2d_noop, 6);
    c2d_fn!("resetTransform", canvas2d_noop, 0);
    c2d_fn!("beginPath", canvas2d_noop, 0);
    c2d_fn!("closePath", canvas2d_noop, 0);
    c2d_fn!("moveTo", canvas2d_noop, 2);
    c2d_fn!("lineTo", canvas2d_noop, 2);
    c2d_fn!("rect", canvas2d_noop, 4);
    c2d_fn!("arc", canvas2d_noop, 6);
    c2d_fn!("fill", canvas2d_noop, 0);
    c2d_fn!("stroke", canvas2d_noop, 0);
    c2d_fn!("clip", canvas2d_noop, 0);
    c2d_fn!("measureText", canvas2d_measure_text, 1);

    let _ = qjs::JS_SetPropertyStr(
        ctx,
        c2d,
        b"fillStyle\0".as_ptr() as *const c_char,
        qjs::JS_NewStringLen(ctx, b"#ffffff\0".as_ptr() as *const c_char, 7),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        c2d,
        b"strokeStyle\0".as_ptr() as *const c_char,
        qjs::JS_NewStringLen(ctx, b"#000000\0".as_ptr() as *const c_char, 7),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        c2d,
        b"globalAlpha\0".as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, 1.0),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        c2d,
        b"canvas\0".as_ptr() as *const c_char,
        qjs::js_dup_value(ctx, canvas),
    );
    c2d
}

unsafe extern "C" fn gl_get_error(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    js_int32(0)
}

unsafe extern "C" fn gl_is_context_lost(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    js_bool(false)
}

unsafe extern "C" fn gl_get_supported_extensions(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let arr = qjs::JS_NewArray(ctx);
    if arr.is_exception() {
        return arr;
    }
    let v0 = qjs::JS_NewStringLen(ctx, b"OES_vertex_array_object\0".as_ptr() as *const c_char, 23);
    let v1 = qjs::JS_NewStringLen(ctx, b"OES_element_index_uint\0".as_ptr() as *const c_char, 22);
    let v2 = qjs::JS_NewStringLen(ctx, b"ANGLE_instanced_arrays\0".as_ptr() as *const c_char, 22);
    let v3 = qjs::JS_NewStringLen(ctx, b"EXT_texture_filter_anisotropic\0".as_ptr() as *const c_char, 30);
    let _ = qjs::JS_SetPropertyUint32(ctx, arr, 0, v0);
    let _ = qjs::JS_SetPropertyUint32(ctx, arr, 1, v1);
    let _ = qjs::JS_SetPropertyUint32(ctx, arr, 2, v2);
    let _ = qjs::JS_SetPropertyUint32(ctx, arr, 3, v3);
    arr
}

unsafe extern "C" fn gl_get_info_log(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs::JS_NewStringLen(ctx, b"\0".as_ptr() as *const c_char, 0)
}

unsafe extern "C" fn gl_get_shader_precision_format(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let o = qjs::JS_NewObject(ctx);
    if o.is_exception() {
        return o;
    }
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"rangeMin\0".as_ptr() as *const c_char, js_int32(127));
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"rangeMax\0".as_ptr() as *const c_char, js_int32(127));
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"precision\0".as_ptr() as *const c_char, js_int32(23));
    o
}

unsafe extern "C" fn gl_check_framebuffer_status(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    js_int32(0x8CD5)
}

unsafe extern "C" fn gl_get_context_attributes(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let o = qjs::JS_NewObject(ctx);
    if o.is_exception() {
        return o;
    }
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"alpha\0".as_ptr() as *const c_char, js_bool(false));
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"antialias\0".as_ptr() as *const c_char, js_bool(false));
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"stencil\0".as_ptr() as *const c_char, js_bool(false));
    let _ = qjs::JS_SetPropertyStr(ctx, o, b"depth\0".as_ptr() as *const c_char, js_bool(false));
    o
}

pub unsafe extern "C" fn canvas_get_context(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let kind_c = qjs::js_to_cstring(ctx, args[0]);
    if kind_c.is_null() {
        return js_null();
    }
    let kind = CStr::from_ptr(kind_c).to_bytes();
    if kind.eq_ignore_ascii_case(b"2d") {
        qjs::JS_FreeCString(ctx, kind_c);
        let existing2d = qjs::JS_GetPropertyStr(ctx, this_val, b"__trueos_2d_ctx\0".as_ptr() as *const c_char);
        if !existing2d.is_exception() && existing2d.tag != qjs::JS_TAG_UNDEFINED && existing2d.tag != qjs::JS_TAG_NULL {
            return existing2d;
        }
        qjs::js_free_value(ctx, existing2d);
        let c2d = make_canvas_2d_context(ctx, this_val);
        if c2d.is_exception() {
            return c2d;
        }
        let keep = qjs::js_dup_value(ctx, c2d);
        let _ = qjs::JS_SetPropertyStr(ctx, this_val, b"__trueos_2d_ctx\0".as_ptr() as *const c_char, keep);
        return c2d;
    }

    let ok = kind.eq_ignore_ascii_case(b"webgl")
        || kind.eq_ignore_ascii_case(b"webgl2")
        || kind.eq_ignore_ascii_case(b"experimental-webgl");
    qjs::JS_FreeCString(ctx, kind_c);
    if !ok {
        return js_null();
    }

    let existing = qjs::JS_GetPropertyStr(ctx, this_val, b"__trueos_gl_ctx\0".as_ptr() as *const c_char);
    if !existing.is_exception() && existing.tag != qjs::JS_TAG_UNDEFINED && existing.tag != qjs::JS_TAG_NULL {
        return existing;
    }
    qjs::js_free_value(ctx, existing);

    let gl = qjs::JS_NewObject(ctx);
    if gl.is_exception() {
        return gl;
    }

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

    gl_fn!("createBuffer", gl_create_buffer, 0);
    gl_fn!("bindBuffer", gl_bind_buffer, 2);
    gl_fn!("bufferData", gl_buffer_data, 3);
    gl_fn!("bufferSubData", gl_buffer_sub_data, 3);
    gl_fn!("createProgram", gl_create_program, 0);
    gl_fn!("useProgram", gl_use_program, 1);
    gl_fn!("createShader", gl_create_shader, 1);
    gl_fn!("deleteShader", gl_noop, 1);
    gl_fn!("shaderSource", gl_shader_source, 2);
    gl_fn!("compileShader", gl_compile_shader, 1);
    gl_fn!("attachShader", gl_attach_shader, 2);
    gl_fn!("bindAttribLocation", gl_bind_attrib_location, 3);
    gl_fn!("linkProgram", gl_link_program, 1);
    gl_fn!("deleteProgram", gl_noop, 1);
    gl_fn!("getShaderParameter", gl_get_shader_program_parameter, 2);
    gl_fn!("getProgramParameter", gl_get_shader_program_parameter, 2);
    gl_fn!("getShaderInfoLog", gl_get_info_log, 1);
    gl_fn!("getProgramInfoLog", gl_get_info_log, 1);
    gl_fn!("getActiveAttrib", gl_get_active_attrib, 2);
    gl_fn!("getActiveUniform", gl_get_active_uniform, 2);
    gl_fn!("getShaderPrecisionFormat", gl_get_shader_precision_format, 2);
    gl_fn!("getAttribLocation", gl_get_attrib_location, 2);
    gl_fn!("enableVertexAttribArray", gl_enable_vertex_attrib_array, 1);
    gl_fn!("disableVertexAttribArray", gl_disable_vertex_attrib_array, 1);
    gl_fn!("vertexAttribPointer", gl_vertex_attrib_pointer, 6);
    gl_fn!("getUniformLocation", gl_get_uniform_location, 2);
    gl_fn!("uniformMatrix3fv", gl_uniform_matrix3fv, 3);
    gl_fn!("uniformMatrix2fv", gl_uniform_matrix2fv, 3);
    gl_fn!("uniformMatrix4fv", gl_uniform_matrix4fv, 3);
    gl_fn!("uniform1f", gl_uniform_1f, 2);
    gl_fn!("uniform2f", gl_uniform_2f, 3);
    gl_fn!("uniform3f", gl_uniform_3f, 4);
    gl_fn!("uniform4f", gl_uniform_4f, 5);
    gl_fn!("uniform1i", gl_uniform_1i, 2);
    gl_fn!("uniform2i", gl_uniform_2i, 3);
    gl_fn!("uniform3i", gl_uniform_3i, 4);
    gl_fn!("uniform4i", gl_uniform_4i, 5);
    gl_fn!("uniform1fv", gl_uniform_1fv, 2);
    gl_fn!("uniform2fv", gl_uniform_2fv, 2);
    gl_fn!("uniform3fv", gl_uniform_3fv, 2);
    gl_fn!("uniform4fv", gl_uniform_4fv, 2);
    gl_fn!("uniform1iv", gl_uniform_1iv, 2);
    gl_fn!("uniform2iv", gl_uniform_2iv, 2);
    gl_fn!("uniform3iv", gl_uniform_3iv, 2);
    gl_fn!("uniform4iv", gl_uniform_4iv, 2);
    gl_fn!("uniform1ui", gl_uniform_1ui, 2);
    gl_fn!("uniform2ui", gl_uniform_2ui, 3);
    gl_fn!("uniform3ui", gl_uniform_3ui, 4);
    gl_fn!("uniform4ui", gl_uniform_4ui, 5);
    gl_fn!("uniform1uiv", gl_uniform_1uiv, 2);
    gl_fn!("uniform2uiv", gl_uniform_2uiv, 2);
    gl_fn!("uniform3uiv", gl_uniform_3uiv, 2);
    gl_fn!("uniform4uiv", gl_uniform_4uiv, 2);
    gl_fn!("clearColor", gl_clear_color, 4);
    gl_fn!("clear", gl_clear, 1);
    gl_fn!("viewport", gl_viewport, 4);
    gl_fn!("enable", gl_enable, 1);
    gl_fn!("disable", gl_disable, 1);
    gl_fn!("blendFunc", gl_noop, 2);
    gl_fn!("blendFuncSeparate", gl_noop, 4);
    gl_fn!("blendEquation", gl_noop, 1);
    gl_fn!("blendEquationSeparate", gl_noop, 2);
    gl_fn!("blendColor", gl_noop, 4);
    gl_fn!("frontFace", gl_noop, 1);
    gl_fn!("cullFace", gl_noop, 1);
    gl_fn!("drawElements", gl_draw_elements, 4);
    gl_fn!("drawArrays", gl_draw_arrays, 3);
    gl_fn!("flush", gl_flush_frame, 0);
    gl_fn!("finish", gl_flush_frame, 0);
    gl_fn!("getError", gl_get_error, 0);
    gl_fn!("isContextLost", gl_is_context_lost, 0);
    gl_fn!("getSupportedExtensions", gl_get_supported_extensions, 0);
    gl_fn!("createTexture", gl_create_texture, 0);
    gl_fn!("deleteTexture", gl_noop, 1);
    gl_fn!("bindTexture", gl_bind_texture, 2);
    gl_fn!("activeTexture", gl_noop, 1);
    gl_fn!("generateMipmap", gl_noop, 1);
    gl_fn!("pixelStorei", gl_pixel_storei, 2);
    gl_fn!("texParameteri", gl_noop, 3);
    gl_fn!("texParameterf", gl_noop, 3);
    gl_fn!("texImage2D", gl_tex_image_2d, 9);
    gl_fn!("texSubImage2D", gl_tex_sub_image_2d, 9);
    gl_fn!("createFramebuffer", gl_create_buffer, 0);
    gl_fn!("bindFramebuffer", gl_noop, 2);
    gl_fn!("deleteFramebuffer", gl_noop, 1);
    gl_fn!("framebufferTexture2D", gl_noop, 5);
    gl_fn!("checkFramebufferStatus", gl_check_framebuffer_status, 1);
    gl_fn!("createRenderbuffer", gl_create_buffer, 0);
    gl_fn!("bindRenderbuffer", gl_noop, 2);
    gl_fn!("deleteRenderbuffer", gl_noop, 1);
    gl_fn!("renderbufferStorage", gl_noop, 4);
    gl_fn!("framebufferRenderbuffer", gl_noop, 4);
    gl_fn!("deleteBuffer", gl_noop, 1);
    gl_fn!("scissor", gl_noop, 4);
    gl_fn!("colorMask", gl_noop, 4);
    gl_fn!("depthMask", gl_noop, 1);
    gl_fn!("depthFunc", gl_noop, 1);
    gl_fn!("clearDepth", gl_noop, 1);
    gl_fn!("polygonOffset", gl_noop, 2);
    gl_fn!("lineWidth", gl_noop, 1);
    gl_fn!("stencilMask", gl_noop, 1);
    gl_fn!("stencilMaskSeparate", gl_noop, 2);
    gl_fn!("stencilFunc", gl_noop, 3);
    gl_fn!("stencilFuncSeparate", gl_noop, 4);
    gl_fn!("stencilOp", gl_noop, 3);
    gl_fn!("stencilOpSeparate", gl_noop, 4);
    gl_fn!("getParameter", gl_get_parameter, 1);
    gl_fn!("getExtension", gl_get_extension, 1);
    gl_fn!("getContextAttributes", gl_get_context_attributes, 0);

    set_i32_const(ctx, gl, b"ARRAY_BUFFER\0", GL_ARRAY_BUFFER as i32);
    set_i32_const(ctx, gl, b"ELEMENT_ARRAY_BUFFER\0", GL_ELEMENT_ARRAY_BUFFER as i32);
    set_i32_const(ctx, gl, b"STATIC_DRAW\0", 0x88E4);
    set_i32_const(ctx, gl, b"DYNAMIC_DRAW\0", 0x88E8);
    set_i32_const(ctx, gl, b"FLOAT\0", GL_FLOAT as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_BYTE\0", GL_UNSIGNED_BYTE as i32);
    set_i32_const(ctx, gl, b"RGBA\0", GL_RGBA as i32);
    set_i32_const(ctx, gl, b"UNPACK_ALIGNMENT\0", GL_UNPACK_ALIGNMENT as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_SHORT\0", GL_UNSIGNED_SHORT as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_INT\0", GL_UNSIGNED_INT as i32);
    set_i32_const(ctx, gl, b"TRIANGLES\0", GL_TRIANGLES as i32);
    set_i32_const(ctx, gl, b"TRIANGLE_STRIP\0", GL_TRIANGLE_STRIP as i32);
    set_i32_const(ctx, gl, b"TRIANGLE_FAN\0", GL_TRIANGLE_FAN as i32);
    set_i32_const(ctx, gl, b"BLEND\0", GL_BLEND as i32);
    set_i32_const(ctx, gl, b"COLOR_BUFFER_BIT\0", GL_COLOR_BUFFER_BIT as i32);
    set_i32_const(ctx, gl, b"ONE\0", 1);
    set_i32_const(ctx, gl, b"ZERO\0", 0);
    set_i32_const(ctx, gl, b"SRC_ALPHA\0", 0x0302);
    set_i32_const(ctx, gl, b"ONE_MINUS_SRC_ALPHA\0", 0x0303);
    set_i32_const(ctx, gl, b"FUNC_ADD\0", 0x8006);
    set_i32_const(ctx, gl, b"CULL_FACE\0", 0x0B44);
    set_i32_const(ctx, gl, b"FRONT\0", 0x0404);
    set_i32_const(ctx, gl, b"BACK\0", 0x0405);
    set_i32_const(ctx, gl, b"FRAMEBUFFER\0", 0x8D40);
    set_i32_const(ctx, gl, b"RENDERBUFFER\0", 0x8D41);
    set_i32_const(ctx, gl, b"FRAMEBUFFER_COMPLETE\0", 0x8CD5);
    set_i32_const(ctx, gl, b"COLOR_ATTACHMENT0\0", 0x8CE0);
    set_i32_const(ctx, gl, b"TEXTURE_2D\0", 0x0DE1);
    set_i32_const(ctx, gl, b"TEXTURE0\0", 0x84C0);
    set_i32_const(ctx, gl, b"CW\0", 0x0900);
    set_i32_const(ctx, gl, b"CCW\0", 0x0901);
    set_i32_const(ctx, gl, b"COMPILE_STATUS\0", GL_COMPILE_STATUS as i32);
    set_i32_const(ctx, gl, b"LINK_STATUS\0", GL_LINK_STATUS as i32);
    set_i32_const(ctx, gl, b"ACTIVE_ATTRIBUTES\0", GL_ACTIVE_ATTRIBUTES as i32);
    set_i32_const(ctx, gl, b"ACTIVE_UNIFORMS\0", GL_ACTIVE_UNIFORMS as i32);
    set_i32_const(ctx, gl, b"VERSION\0", GL_VERSION as i32);
    set_i32_const(ctx, gl, b"RENDERER\0", GL_RENDERER as i32);
    set_i32_const(ctx, gl, b"VENDOR\0", GL_VENDOR as i32);
    set_i32_const(ctx, gl, b"FRAGMENT_SHADER\0", GL_FRAGMENT_SHADER as i32);
    set_i32_const(ctx, gl, b"VERTEX_SHADER\0", GL_VERTEX_SHADER as i32);
    set_i32_const(ctx, gl, b"MAX_VERTEX_ATTRIBS\0", GL_MAX_VERTEX_ATTRIBS as i32);
    set_i32_const(ctx, gl, b"MAX_TEXTURE_IMAGE_UNITS\0", GL_MAX_TEXTURE_IMAGE_UNITS as i32);
    set_i32_const(ctx, gl, b"MAX_COMBINED_TEXTURE_IMAGE_UNITS\0", GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS as i32);
    set_i32_const(ctx, gl, b"STENCIL_BITS\0", GL_STENCIL_BITS as i32);
    set_i32_const(ctx, gl, b"HIGH_FLOAT\0", GL_HIGH_FLOAT as i32);
    set_i32_const(ctx, gl, b"MEDIUM_FLOAT\0", GL_MEDIUM_FLOAT as i32);
    set_i32_const(ctx, gl, b"LOW_FLOAT\0", GL_LOW_FLOAT as i32);
    set_i32_const(ctx, gl, b"HIGH_INT\0", GL_HIGH_INT as i32);
    set_i32_const(ctx, gl, b"MEDIUM_INT\0", GL_MEDIUM_INT as i32);
    set_i32_const(ctx, gl, b"LOW_INT\0", GL_LOW_INT as i32);
    set_i32_const(ctx, gl, b"FLOAT_VEC2\0", GL_FLOAT_VEC2 as i32);
    set_i32_const(ctx, gl, b"FLOAT_VEC3\0", GL_FLOAT_VEC3 as i32);
    set_i32_const(ctx, gl, b"FLOAT_VEC4\0", GL_FLOAT_VEC4 as i32);
    set_i32_const(ctx, gl, b"INT\0", GL_INT as i32);
    set_i32_const(ctx, gl, b"INT_VEC2\0", GL_INT_VEC2 as i32);
    set_i32_const(ctx, gl, b"INT_VEC3\0", GL_INT_VEC3 as i32);
    set_i32_const(ctx, gl, b"INT_VEC4\0", GL_INT_VEC4 as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_INT\0", GL_UNSIGNED_INT as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_INT_VEC2\0", GL_UNSIGNED_INT_VEC2 as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_INT_VEC3\0", GL_UNSIGNED_INT_VEC3 as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_INT_VEC4\0", GL_UNSIGNED_INT_VEC4 as i32);
    set_i32_const(ctx, gl, b"BOOL\0", GL_BOOL as i32);
    set_i32_const(ctx, gl, b"BOOL_VEC2\0", GL_BOOL_VEC2 as i32);
    set_i32_const(ctx, gl, b"BOOL_VEC3\0", GL_BOOL_VEC3 as i32);
    set_i32_const(ctx, gl, b"BOOL_VEC4\0", GL_BOOL_VEC4 as i32);
    set_i32_const(ctx, gl, b"FLOAT_MAT2\0", GL_FLOAT_MAT2 as i32);
    set_i32_const(ctx, gl, b"FLOAT_MAT3\0", GL_FLOAT_MAT3 as i32);
    set_i32_const(ctx, gl, b"FLOAT_MAT4\0", GL_FLOAT_MAT4 as i32);
    set_i32_const(ctx, gl, b"SAMPLER_2D\0", GL_SAMPLER_2D as i32);
    set_i32_const(ctx, gl, b"INT_SAMPLER_2D\0", GL_INT_SAMPLER_2D as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_INT_SAMPLER_2D\0", GL_UNSIGNED_INT_SAMPLER_2D as i32);
    set_i32_const(ctx, gl, b"SAMPLER_CUBE\0", GL_SAMPLER_CUBE as i32);
    set_i32_const(ctx, gl, b"INT_SAMPLER_CUBE\0", GL_INT_SAMPLER_CUBE as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_INT_SAMPLER_CUBE\0", GL_UNSIGNED_INT_SAMPLER_CUBE as i32);
    set_i32_const(ctx, gl, b"SAMPLER_2D_ARRAY\0", GL_SAMPLER_2D_ARRAY as i32);
    set_i32_const(ctx, gl, b"INT_SAMPLER_2D_ARRAY\0", GL_INT_SAMPLER_2D_ARRAY as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_INT_SAMPLER_2D_ARRAY\0", GL_UNSIGNED_INT_SAMPLER_2D_ARRAY as i32);

    let mut w = 1280.0f64;
    let mut h = 800.0f64;
    let wv = qjs::JS_GetPropertyStr(ctx, this_val, b"width\0".as_ptr() as *const c_char);
    let hv = qjs::JS_GetPropertyStr(ctx, this_val, b"height\0".as_ptr() as *const c_char);
    let _ = qjs::JS_ToFloat64(ctx, &mut w as *mut f64, wv);
    let _ = qjs::JS_ToFloat64(ctx, &mut h as *mut f64, hv);
    qjs::js_free_value(ctx, wv);
    qjs::js_free_value(ctx, hv);
    let _ = qjs::JS_SetPropertyStr(ctx, gl, b"drawingBufferWidth\0".as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, w.max(1.0)));
    let _ = qjs::JS_SetPropertyStr(ctx, gl, b"drawingBufferHeight\0".as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, h.max(1.0)));

    {
        let mut st = GL_STATE.lock();
        st.viewport_w = w.max(1.0) as i32;
        st.viewport_h = h.max(1.0) as i32;
    }

    let keep = qjs::js_dup_value(ctx, gl);
    let _ = qjs::JS_SetPropertyStr(ctx, this_val, b"__trueos_gl_ctx\0".as_ptr() as *const c_char, keep);
    gl
}
