extern crate alloc;

use alloc::vec::Vec;
use core::ffi::{c_char, c_int, CStr};
use spin::Mutex;

use crate as qjs;
use crate::cmd_stream;

const MAX_ATTRS: usize = 16;

const GL_FLOAT: u32 = 0x1406;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_UNSIGNED_SHORT: u32 = 0x1403;
const GL_UNSIGNED_INT: u32 = 0x1405;

const GL_TRIANGLES: u32 = 0x0004;
const GL_TRIANGLE_STRIP: u32 = 0x0005;
const GL_TRIANGLE_FAN: u32 = 0x0006;

const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_ELEMENT_ARRAY_BUFFER: u32 = 0x8893;

const GL_BLEND: u32 = 0x0BE2;
const GL_COLOR_BUFFER_BIT: u32 = 0x0000_4000;

const GL_COMPILE_STATUS: u32 = 0x8B81;
const GL_LINK_STATUS: u32 = 0x8B82;

const GL_VERSION: u32 = 0x1F02;
const GL_RENDERER: u32 = 0x1F01;
const GL_VENDOR: u32 = 0x1F00;
const GL_MAX_VERTEX_ATTRIBS: u32 = 0x8869;

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
}

impl ProgramState {
    fn new() -> Self {
        Self {
            attrib_names: Vec::new(),
            uniform_names: Vec::new(),
            uniform_mat3: Vec::new(),
        }
    }
}

struct GlState {
    next_handle: u32,
    buffers: Vec<Option<Vec<u8>>>,
    programs: Vec<Option<ProgramState>>,
    current_array_buffer: u32,
    current_element_array_buffer: u32,
    current_program: u32,
    attribs: [AttribState; MAX_ATTRS],
    clear_rgb: u32,
    viewport_w: i32,
    viewport_h: i32,
    blend_enabled: bool,
}

impl GlState {
    const fn new() -> Self {
        Self {
            next_handle: 1,
            buffers: Vec::new(),
            programs: Vec::new(),
            current_array_buffer: 0,
            current_element_array_buffer: 0,
            current_program: 0,
            attribs: [attrib_default(); MAX_ATTRS],
            clear_rgb: 0x12161d,
            viewport_w: 1280,
            viewport_h: 800,
            blend_enabled: false,
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

fn emit_triangles(st: &GlState, indices: &[u32]) {
    let mut pos_attr = None;
    for i in 0..MAX_ATTRS {
        let a = st.attribs[i];
        if a.enabled && a.buffer_id != 0 && a.size >= 2 {
            pos_attr = Some(a);
            break;
        }
    }
    let Some(pa) = pos_attr else {
        return;
    };
    let Some(Some(vb)) = st.buffers.get((pa.buffer_id - 1) as usize) else {
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

    let mut out = Vec::with_capacity(indices.len().saturating_mul(12));
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
        let (nx, ny) = transform_xy(st, x, y);
        pack_vertex(&mut out, nx, ny, 232, 140, 40);
    }
    if out.is_empty() {
        return;
    }
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetViewport {
        w: st.viewport_w.max(1),
        h: st.viewport_h.max(1),
    });
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetClearColor {
        clear_rgb: st.clear_rgb,
    });
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetBlendEnabled {
        enabled: st.blend_enabled,
    });
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::BeginFrame);
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::DrawTriangles { vertices: out });
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::EndFrame);
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
    st.buffers[idx] = Some(Vec::new());
    js_new_handle_obj(ctx, id)
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
    let name = CStr::from_ptr(name_c).to_bytes().to_vec();
    qjs::JS_FreeCString(ctx, name_c);
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
    if loc_obj.tag != qjs::JS_TAG_OBJECT {
        return qjs::JSValue::undefined();
    }
    let v = qjs::JS_GetPropertyStr(ctx, loc_obj, b"__u\0".as_ptr() as *const c_char);
    let loc = js_get_f64(ctx, v).map(|x| x.max(0.0) as usize).unwrap_or(usize::MAX);
    qjs::js_free_value(ctx, v);
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
    let curr_prog = st.current_program;
    let Some(p) = program_slot_mut(&mut st, curr_prog) else {
        return qjs::JSValue::undefined();
    };
    if loc < p.uniform_mat3.len() {
        p.uniform_mat3[loc] = Some(m);
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
    GL_STATE.lock().clear_rgb = (r << 16) | (g << 8) | b;
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
        let st = GL_STATE.lock();
        cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetViewport {
            w: st.viewport_w.max(1),
            h: st.viewport_h.max(1),
        });
        cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetClearColor { clear_rgb: st.clear_rgb });
        cmd_stream::enqueue(cmd_stream::CmdStreamCommand::BeginFrame);
        cmd_stream::enqueue(cmd_stream::CmdStreamCommand::EndFrame);
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
    st.viewport_w = w.max(1);
    st.viewport_h = h.max(1);
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
        GL_STATE.lock().blend_enabled = enabled;
    }
    qjs::JSValue::undefined()
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
    let st = GL_STATE.lock();
    let elem_id = st.current_element_array_buffer;
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
    if idx_src.len() < 3 {
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
        emit_triangles(&st, tri.as_slice());
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
    let st = GL_STATE.lock();
    emit_triangles(&st, idx.as_slice());
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
    let pname = js_get_f64(ctx, args[1]).unwrap_or(0.0).max(0.0) as u32;
    if pname == GL_COMPILE_STATUS || pname == GL_LINK_STATUS {
        return js_bool(true);
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
        || name.eq_ignore_ascii_case(b"ANGLE_instanced_arrays")
    {
        return qjs::JS_NewObject(ctx);
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
    gl_fn!("createShader", gl_create_buffer, 1);
    gl_fn!("shaderSource", gl_noop, 2);
    gl_fn!("compileShader", gl_noop, 1);
    gl_fn!("attachShader", gl_noop, 2);
    gl_fn!("linkProgram", gl_noop, 1);
    gl_fn!("getShaderParameter", gl_get_shader_program_parameter, 2);
    gl_fn!("getProgramParameter", gl_get_shader_program_parameter, 2);
    gl_fn!("getShaderInfoLog", gl_noop, 1);
    gl_fn!("getProgramInfoLog", gl_noop, 1);
    gl_fn!("getAttribLocation", gl_get_attrib_location, 2);
    gl_fn!("enableVertexAttribArray", gl_enable_vertex_attrib_array, 1);
    gl_fn!("disableVertexAttribArray", gl_disable_vertex_attrib_array, 1);
    gl_fn!("vertexAttribPointer", gl_vertex_attrib_pointer, 6);
    gl_fn!("getUniformLocation", gl_get_uniform_location, 2);
    gl_fn!("uniformMatrix3fv", gl_uniform_matrix3fv, 3);
    gl_fn!("uniformMatrix4fv", gl_noop, 3);
    gl_fn!("uniform1f", gl_noop, 2);
    gl_fn!("uniform1i", gl_noop, 2);
    gl_fn!("uniform2f", gl_noop, 3);
    gl_fn!("uniform4f", gl_noop, 5);
    gl_fn!("uniform4fv", gl_noop, 2);
    gl_fn!("clearColor", gl_clear_color, 4);
    gl_fn!("clear", gl_clear, 1);
    gl_fn!("viewport", gl_viewport, 4);
    gl_fn!("enable", gl_enable, 1);
    gl_fn!("disable", gl_disable, 1);
    gl_fn!("blendFunc", gl_noop, 2);
    gl_fn!("blendFuncSeparate", gl_noop, 4);
    gl_fn!("blendEquation", gl_noop, 1);
    gl_fn!("blendEquationSeparate", gl_noop, 2);
    gl_fn!("frontFace", gl_noop, 1);
    gl_fn!("cullFace", gl_noop, 1);
    gl_fn!("drawElements", gl_draw_elements, 4);
    gl_fn!("drawArrays", gl_draw_arrays, 3);
    gl_fn!("flush", gl_noop, 0);
    gl_fn!("finish", gl_noop, 0);
    gl_fn!("createTexture", gl_create_buffer, 0);
    gl_fn!("bindTexture", gl_noop, 2);
    gl_fn!("activeTexture", gl_noop, 1);
    gl_fn!("pixelStorei", gl_noop, 2);
    gl_fn!("texParameteri", gl_noop, 3);
    gl_fn!("texImage2D", gl_noop, 9);
    gl_fn!("texSubImage2D", gl_noop, 9);
    gl_fn!("getParameter", gl_get_parameter, 1);
    gl_fn!("getExtension", gl_get_extension, 1);
    gl_fn!("getContextAttributes", gl_get_context_attributes, 0);

    set_i32_const(ctx, gl, b"ARRAY_BUFFER\0", GL_ARRAY_BUFFER as i32);
    set_i32_const(ctx, gl, b"ELEMENT_ARRAY_BUFFER\0", GL_ELEMENT_ARRAY_BUFFER as i32);
    set_i32_const(ctx, gl, b"STATIC_DRAW\0", 0x88E4);
    set_i32_const(ctx, gl, b"DYNAMIC_DRAW\0", 0x88E8);
    set_i32_const(ctx, gl, b"FLOAT\0", GL_FLOAT as i32);
    set_i32_const(ctx, gl, b"UNSIGNED_BYTE\0", GL_UNSIGNED_BYTE as i32);
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
    set_i32_const(ctx, gl, b"CW\0", 0x0900);
    set_i32_const(ctx, gl, b"CCW\0", 0x0901);
    set_i32_const(ctx, gl, b"COMPILE_STATUS\0", GL_COMPILE_STATUS as i32);
    set_i32_const(ctx, gl, b"LINK_STATUS\0", GL_LINK_STATUS as i32);
    set_i32_const(ctx, gl, b"VERSION\0", GL_VERSION as i32);
    set_i32_const(ctx, gl, b"RENDERER\0", GL_RENDERER as i32);
    set_i32_const(ctx, gl, b"VENDOR\0", GL_VENDOR as i32);
    set_i32_const(ctx, gl, b"MAX_VERTEX_ATTRIBS\0", GL_MAX_VERTEX_ATTRIBS as i32);

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
