#![cfg(feature = "trueos")]

use alloc::vec::Vec;
use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;
use crate::browser_canvas::SIMPLE_DOM_CANVAS_SHIM_JS;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_gfx_present_owner_set(owner: u32);
    fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_draw_rgb_triangles_no_present(vtx_ptr: *const u8, vtx_len: usize) -> i32;
    fn trueos_cabi_gfx_end_frame() -> i32;
}

static WEBGPU_BROWSER_TASK_STARTED: AtomicBool = AtomicBool::new(false);

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

#[inline]
fn push_rgb_vertex(buf: &mut Vec<u8>, x: f32, y: f32, r: u8, g: u8, b: u8) {
    buf.extend_from_slice(&x.to_le_bytes());
    buf.extend_from_slice(&y.to_le_bytes());
    buf.push(r);
    buf.push(g);
    buf.push(b);
    buf.push(0);
}

fn submit_boot_visual_checkpoint() {
    // Deterministic checkpoint: emit opaque on-screen RGB geometry directly via
    // CABI so scanout visibility can be validated independently from JS replay.
    let mut vtx = Vec::with_capacity(12 * 6);
    // Screen-filling quad in NDC (two triangles) with vivid green.
    push_rgb_vertex(&mut vtx, -1.0, -1.0, 20, 255, 20);
    push_rgb_vertex(&mut vtx, 1.0, -1.0, 20, 255, 20);
    push_rgb_vertex(&mut vtx, 1.0, 1.0, 20, 255, 20);
    push_rgb_vertex(&mut vtx, -1.0, -1.0, 20, 255, 20);
    push_rgb_vertex(&mut vtx, 1.0, 1.0, 20, 255, 20);
    push_rgb_vertex(&mut vtx, -1.0, 1.0, 20, 255, 20);

    let (rc_begin, rc_draw, rc_end) = unsafe {
        let rc_begin = trueos_cabi_gfx_begin_frame(0xFFFFFF);
        let rc_draw = trueos_cabi_gfx_draw_rgb_triangles_no_present(vtx.as_ptr(), vtx.len());
        let rc_end = trueos_cabi_gfx_end_frame();
        (rc_begin, rc_draw, rc_end)
    };

    let mut msg = Vec::with_capacity(96);
    msg.extend_from_slice(b"qjs-browser: boot-visual-checkpoint begin=");
    push_i32_ascii(&mut msg, rc_begin);
    msg.extend_from_slice(b" draw=");
    push_i32_ascii(&mut msg, rc_draw);
    msg.extend_from_slice(b" end=");
    push_i32_ascii(&mut msg, rc_end);
    msg.extend_from_slice(b"\n");
    log_bytes(&msg);
}

fn push_i32_ascii(out: &mut Vec<u8>, value: i32) {
    let mut buf = [0u8; 12];
    let mut n = value as i64;
    let neg = n < 0;
    if neg {
        n = -n;
    }
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    out.extend_from_slice(&buf[i..]);
}

unsafe fn drain_pending_jobs(rt: *mut qjs::JSRuntime, fallback_ctx: *mut qjs::JSContext) -> bool {
    if rt.is_null() {
        return true;
    }
    loop {
        let mut job_ctx: *mut qjs::JSContext = core::ptr::null_mut();
        let rc = qjs::JS_ExecutePendingJob(rt, &mut job_ctx as *mut *mut qjs::JSContext);
        if rc > 0 {
            continue;
        }
        if rc < 0 {
            let ctx = if !job_ctx.is_null() {
                job_ctx
            } else {
                fallback_ctx
            };
            if !ctx.is_null() {
                qjs::qjs_diag::dump_last_exception(ctx, "pixi-browser pending-job");
            }
            return false;
        }
        break;
    }
    true
}

unsafe fn pump_runtime_once(rt: *mut qjs::JSRuntime, ctx: *mut qjs::JSContext) -> bool {
    let mut progress = false;
    progress |= qjs::async_ops::pump(ctx);
    progress |= qjs::workers::pump(ctx);
    progress |= qjs::timers::pump(ctx);
    if !drain_pending_jobs(rt, ctx) {
        return false;
    }
    if qjs::JS_IsJobPending(rt) > 0
        || qjs::async_ops::has_pending(ctx)
        || qjs::workers::has_pending_for_ctx(ctx)
    {
        qjs::trueos_shims::trueos_cabi_poll_once();
        if !progress {
            qjs::trueos_shims::trueos_cabi_poll_once();
        }
    }
    true
}

unsafe fn eval_or_log(
    ctx: *mut qjs::JSContext,
    src: &[u8],
    filename: *const c_char,
    flags: i32,
    label: &str,
) -> bool {
    let val = qjs::js_eval_bytes(ctx, src, filename, flags);
    if val.is_exception() {
        log_str("qjs-browser: ");
        log_str(label);
        log_str(" JS_Eval exception\n");
        qjs::qjs_diag::dump_last_exception(ctx, "browser eval");
        return false;
    }
    qjs::js_free_value(ctx, val);
    true
}

#[embassy_executor::task]
pub async fn boot_browser() {
    if WEBGPU_BROWSER_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-browser: already running\n");
        return;
    }

    log_str("qjs-browser: starting (render bridge on)\n");
    unsafe { trueos_cabi_gfx_present_owner_set(1) };
    submit_boot_visual_checkpoint();
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-browser: JS_NewRuntime failed\n");
                trueos_cabi_gfx_present_owner_set(0);
                WEBGPU_BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };
        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        qjs::node::install_globals(ctx);

        // Install native mouse bridge helpers on the global object so JS can poll
        // and dispatch pointer events without needing a browser host runtime.
        let global = qjs::JS_GetGlobalObject(ctx);
        qjs::browser::install_mouse_api(ctx, global);
        qjs::js_free_value(ctx, global);

        let init_filename = b"<pixi-browser-init>\0";
        let mut init_script = Vec::new();
        init_script.extend_from_slice(SIMPLE_DOM_CANVAS_SHIM_JS);
        init_script.extend_from_slice(br#"
const G = (typeof globalThis !== 'undefined') ? globalThis : this;

if (!G.window) G.window = G;
if (typeof G.window.innerWidth !== 'number') G.window.innerWidth = 1280;
if (typeof G.window.innerHeight !== 'number') G.window.innerHeight = 800;
if (!G.window.devicePixelRatio) G.window.devicePixelRatio = 1;
if (!G.performance || typeof G.performance.now !== 'function') {
    const __t0 = Date.now();
    G.performance = {
        now() {
            return Date.now() - __t0;
        },
    };
}
if (typeof G.addEventListener !== 'function') G.addEventListener = () => {};
if (typeof G.removeEventListener !== 'function') G.removeEventListener = () => {};
if (typeof G.dispatchEvent !== 'function') G.dispatchEvent = () => true;
if (!G.requestAnimationFrame) {
    let __trueosRafId = 1;
    const __trueosRafPending = [];
    const __trueosRafCanceled = new Set();
    G.requestAnimationFrame = (cb) => {
        const id = __trueosRafId++;
        __trueosRafPending.push([id, cb]);
        return id;
    };
    G.cancelAnimationFrame = (id) => {
        __trueosRafCanceled.add(Number(id) | 0);
    };
    G.__trueos_raf_pump = () => {
        if (__trueosRafPending.length === 0) return;
        const now = Date.now();
        const batch = __trueosRafPending.splice(0, __trueosRafPending.length);
        for (let i = 0; i < batch.length; i++) {
            const pair = batch[i];
            if (!pair) continue;
            const id = Number(pair[0]) | 0;
            const cb = pair[1];
            if (__trueosRafCanceled.has(id)) {
                __trueosRafCanceled.delete(id);
                continue;
            }
            try {
                if (typeof cb === 'function') cb(now);
            } catch (_) {}
        }
    };
}
if (!G.cancelAnimationFrame) G.cancelAnimationFrame = () => {};

const __trueosWebGpuState = {
    preferredCanvasFormat: 'bgra8unorm',
    backendName: 'trueos-cmd-stream',
    nextId: 1,
    submitCount: 0,
    submittedCmdBuffers: 0,
    submittedPasses: 0,
    submittedOps: 0,
    submittedDraws: 0,
};
G.__trueosWebGpuState = __trueosWebGpuState;
let __trueosCmdStream = null;
try {
    __trueosCmdStream = await import('cmd_stream');
} catch (_) {
    __trueosCmdStream = null;
}
G.__trueosCmdStream = __trueosCmdStream;

const __trueosGpuLimits = {
    maxBindGroups: 4,
    maxBindingsPerBindGroup: 640,
    maxTextureDimension1D: 8192,
    maxTextureDimension2D: 8192,
    maxTextureDimension3D: 2048,
    maxDynamicUniformBuffersPerPipelineLayout: 8,
    maxDynamicStorageBuffersPerPipelineLayout: 4,
    maxStorageBuffersPerShaderStage: 8,
    maxUniformBuffersPerShaderStage: 12,
    maxVertexBuffers: 8,
    maxVertexAttributes: 16,
    maxVertexBufferArrayStride: 2048,
    maxColorAttachments: 4,
};

function __trueosInstallGpuConstants(T) {
    if (!T.GPUBufferUsage) {
        T.GPUBufferUsage = {
            MAP_READ: 0x0001,
            MAP_WRITE: 0x0002,
            COPY_SRC: 0x0004,
            COPY_DST: 0x0008,
            INDEX: 0x0010,
            VERTEX: 0x0020,
            UNIFORM: 0x0040,
            STORAGE: 0x0080,
            INDIRECT: 0x0100,
            QUERY_RESOLVE: 0x0200,
        };
    }
    if (!T.GPUTextureUsage) {
        T.GPUTextureUsage = {
            COPY_SRC: 0x01,
            COPY_DST: 0x02,
            TEXTURE_BINDING: 0x04,
            STORAGE_BINDING: 0x08,
            RENDER_ATTACHMENT: 0x10,
        };
    }
    if (!T.GPUMapMode) {
        T.GPUMapMode = {
            READ: 0x0001,
            WRITE: 0x0002,
        };
    }
    if (!T.GPUShaderStage) {
        T.GPUShaderStage = {
            VERTEX: 0x1,
            FRAGMENT: 0x2,
            COMPUTE: 0x4,
        };
    }
    if (!T.GPUColorWrite) {
        T.GPUColorWrite = {
            RED: 0x1,
            GREEN: 0x2,
            BLUE: 0x4,
            ALPHA: 0x8,
            ALL: 0xF,
        };
    }
}

function __trueosToU8(data) {
    if (!data) return null;
    if (data instanceof Uint8Array) return data;
    if (ArrayBuffer.isView(data)) {
        return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    }
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    return null;
}
G.__trueosToU8 = __trueosToU8;

function __trueosMakeRgbasStorage(width, height, prev) {
    const w = Math.max(1, Number(width || 1) | 0);
    const h = Math.max(1, Number(height || 1) | 0);
    const px = w * h;
    const rgba = new Uint8Array(px * 4);
    const stencil = new Uint8Array(px);

    if (prev && prev.rgba instanceof Uint8Array) {
        const copy = Math.min(prev.rgba.byteLength, rgba.byteLength);
        if (copy > 0) rgba.set(prev.rgba.subarray(0, copy), 0);
    }
    if (prev && prev.stencil instanceof Uint8Array) {
        const copyS = Math.min(prev.stencil.byteLength, stencil.byteLength);
        if (copyS > 0) stencil.set(prev.stencil.subarray(0, copyS), 0);
    }

    return {
        width: w,
        height: h,
        rgba,
        stencil,
    };
}

function __trueosEnsureTextureRgbas(tex) {
    if (!tex) return null;
    const w = Math.max(1, Number(tex.width || 1) | 0);
    const h = Math.max(1, Number(tex.height || 1) | 0);

    const cur = tex.__rgbas;
    const good = cur
        && cur.rgba instanceof Uint8Array
        && cur.stencil instanceof Uint8Array
        && cur.width === w
        && cur.height === h
        && cur.rgba.byteLength === (w * h * 4)
        && cur.stencil.byteLength === (w * h);

    if (!good) {
        tex.__rgbas = __trueosMakeRgbasStorage(w, h, cur);

        // Boundary contract: preserve legacy/raw RGBA visibility only.
        if (tex.__data instanceof Uint8Array && tex.__data.byteLength > 0) {
            const dst = tex.__rgbas.rgba;
            const copy = Math.min(dst.byteLength, tex.__data.byteLength);
            if (copy > 0) dst.set(tex.__data.subarray(0, copy), 0);
        }
    }

    tex.__data = tex.__rgbas.rgba;
    return tex.__rgbas;
}

function __trueosTextureRgbaBytes(tex) {
    const rgbas = __trueosEnsureTextureRgbas(tex);
    return rgbas ? rgbas.rgba : null;
}

function __trueosTextureStencilBytes(tex) {
    const rgbas = __trueosEnsureTextureRgbas(tex);
    return rgbas ? rgbas.stencil : null;
}

function __trueosStripStencilFast(tex) {
    // Zero-copy strip: the RGBA plane remains directly reusable.
    return __trueosTextureRgbaBytes(tex);
}

function __trueosUploadLinearRgbaToTexture(tex, src, bytesPerRow, width, height, originX = 0, originY = 0) {
    const dst = __trueosTextureRgbaBytes(tex);
    if (!dst || !(src instanceof Uint8Array)) return;

    const texW = Math.max(1, Number(tex.width || 1) | 0);
    const texH = Math.max(1, Number(tex.height || 1) | 0);
    const copyW = Math.max(0, Math.min(Number(width) | 0, texW - originX));
    const copyH = Math.max(0, Math.min(Number(height) | 0, texH - originY));
    if (copyW <= 0 || copyH <= 0) return;

    const srcStride = Math.max(copyW * 4, Number(bytesPerRow || (copyW * 4)) | 0);
    const rowBytes = copyW * 4;
    for (let y = 0; y < copyH; y++) {
        const srcOff = y * srcStride;
        const srcEnd = srcOff + rowBytes;
        if (srcEnd > src.byteLength) break;
        const dstOff = ((originY + y) * texW + originX) * 4;
        const dstEnd = dstOff + rowBytes;
        if (dstEnd > dst.byteLength) break;
        dst.set(src.subarray(srcOff, srcEnd), dstOff);
    }

    // Track host-side texture mutations so replay upload bridges can re-sync lazily.
    tex.__rev = (Number(tex.__rev || 0) | 0) + 1;
}
G.__trueosUploadLinearRgbaToTexture = __trueosUploadLinearRgbaToTexture;

function __trueosMakeGpuTexture(label, width, height, format, usage) {
    const w = Math.max(1, Number(width) | 0);
    const h = Math.max(1, Number(height) | 0);
    const rgbas = __trueosMakeRgbasStorage(w, h);
    const tex = {
        __id: __trueosWebGpuState.nextId++,
        label: label || '',
        width: w,
        height: h,
        depthOrArrayLayers: 1,
        format: String(format || __trueosWebGpuState.preferredCanvasFormat),
        usage: (usage == null) ? ((G.GPUTextureUsage && G.GPUTextureUsage.RENDER_ATTACHMENT) || 0x10) : Number(usage),
        __rgbas: rgbas,
        __data: rgbas.rgba,
        __rev: 1,
        destroyed: false,
        createView(desc = {}) {
            return {
                __id: __trueosWebGpuState.nextId++,
                label: String(desc.label || ''),
                texture: tex,
                format: String(desc.format || tex.format),
                dimension: String(desc.dimension || '2d'),
                aspect: String(desc.aspect || 'all'),
            };
        },
        destroy() {
            tex.destroyed = true;
        },
    };
    return tex;
}

function __trueosMakeGpuBuffer(desc = {}) {
    const size = Math.max(0, Number(desc.size || 0) | 0);
    const data = new Uint8Array(size);
    const state = {
        mapped: !!desc.mappedAtCreation,
        mapRanges: [],
    };
    return {
        __id: __trueosWebGpuState.nextId++,
        label: String(desc.label || ''),
        size,
        usage: Number(desc.usage || 0),
        mapState: state.mapped ? 'mapped' : 'unmapped',
        __data: data,
        getMappedRange(offset = 0, length = size) {
            const o = Math.max(0, Number(offset) | 0);
            const n = Math.max(0, Number(length) | 0);
            const end = Math.min(data.byteLength, o + n);
            const tmp = data.buffer.slice(o, end);
            state.mapRanges.push({
                offset: o,
                length: Math.max(0, end - o),
                tmp,
            });
            return tmp;
        },
        unmap() {
            // Commit mapped-range writes back into the backing store.
            for (let i = 0; i < state.mapRanges.length; i++) {
                const r = state.mapRanges[i];
                if (!r || !r.tmp) continue;
                const src = new Uint8Array(r.tmp);
                const o = Math.max(0, Number(r.offset) | 0);
                const len = Math.min(src.byteLength, Math.max(0, Number(r.length) | 0));
                const end = Math.min(data.byteLength, o + len);
                if (end > o) {
                    data.set(src.subarray(0, end - o), o);
                }
            }
            state.mapRanges.length = 0;
            state.mapped = false;
            this.mapState = 'unmapped';
        },
        async mapAsync() {
            state.mapped = true;
            this.mapState = 'mapped';
            return;
        },
        destroy() {
            this.destroyed = true;
            this.mapState = 'destroyed';
        },
    };
}

// Minimal command encoder/queue fallback used when the canvas-renderer shim is
// not present. This keeps WebGPU probes and lightweight command recording alive
// while the rich UI renders through the direct cmd backend path.
if (typeof G.__trueosMakeGpuCommandEncoder !== 'function') {
    function __trueosSummarizeCmdBuffer(cb) {
        const cmds = Array.isArray(cb && cb.__cmds) ? cb.__cmds : [];
        let passes = 0;
        let ops = 0;
        let draws = 0;
        for (let i = 0; i < cmds.length; i++) {
            const cmd = cmds[i];
            if (cmd && cmd.__kind === 'render-pass') {
                passes++;
                const passOps = Array.isArray(cmd.__ops) ? cmd.__ops : [];
                ops += passOps.length;
                for (let j = 0; j < passOps.length; j++) {
                    const op = passOps[j];
                    const name = Array.isArray(op) ? String(op[0] || '') : '';
                    if (name === 'draw' || name === 'drawIndexed')
                        draws++;
                }
                continue;
            }
            if (Array.isArray(cmd))
                ops++;
        }
        return {
            commands: cmds.length,
            passes,
            ops,
            draws,
        };
    }

    G.__trueosMakeGpuCommandEncoder = function __trueosMakeGpuCommandEncoder(_device, desc = {}) {
        const cmds = [];
        return {
            __id: __trueosWebGpuState.nextId++,
            __desc: desc,
            __cmds: cmds,
            beginRenderPass(passDesc = {}) {
                const ops = [];
                const pass = {
                    __id: __trueosWebGpuState.nextId++,
                    __kind: 'render-pass',
                    __desc: passDesc,
                    __ops: ops,
                    setPipeline(p) { ops.push(['setPipeline', p]); },
                    setBindGroup(i, bg) { ops.push(['setBindGroup', i, bg]); },
                    setVertexBuffer(slot, b, off = 0, size) { ops.push(['setVertexBuffer', slot, b, off, size]); },
                    setIndexBuffer(b, f = 'uint16', off = 0, size) { ops.push(['setIndexBuffer', b, f, off, size]); },
                    setViewport(x, y, w, h, minD = 0, maxD = 1) { ops.push(['setViewport', x, y, w, h, minD, maxD]); },
                    setScissorRect(x, y, w, h) { ops.push(['setScissorRect', x, y, w, h]); },
                    setStencilReference(v) { ops.push(['setStencilReference', v]); },
                    setBlendConstant(c) { ops.push(['setBlendConstant', c]); },
                    executeBundles(bundles) { ops.push(['executeBundles', bundles]); },
                    draw(vtxCount, instCount = 1, firstV = 0, firstI = 0) { ops.push(['draw', vtxCount, instCount, firstV, firstI]); },
                    drawIndexed(idxCount, instCount = 1, firstIndex = 0, baseVertex = 0, firstInstance = 0) {
                        ops.push(['drawIndexed', idxCount, instCount, firstIndex, baseVertex, firstInstance]);
                    },
                    end() {},
                };
                cmds.push(pass);
                return pass;
            },
            copyBufferToBuffer(src, srcOffset, dst, dstOffset, size) {
                cmds.push(['copyBufferToBuffer', src, srcOffset, dst, dstOffset, size]);
            },
            copyBufferToTexture(src, dst, size) {
                cmds.push(['copyBufferToTexture', src, dst, size]);
            },
            copyTextureToTexture(src, dst, size) {
                cmds.push(['copyTextureToTexture', src, dst, size]);
            },
            finish(_finishDesc = {}) {
                const summary = __trueosSummarizeCmdBuffer({ __cmds: cmds });
                return {
                    __id: __trueosWebGpuState.nextId++,
                    __cmds: cmds.slice(),
                    __summary: summary,
                };
            },
        };
    };
}

if (typeof G.__trueosMakeGpuQueue !== 'function') {
    function __trueosPackVerts12(verts) {
        const list = Array.isArray(verts) ? verts : [];
        if (list.length <= 0)
            return null;
        const out = new Uint8Array(list.length * 12);
        const dv = new DataView(out.buffer);
        let off = 0;
        for (let i = 0; i < list.length; i++) {
            const v = list[i] || {};
            dv.setFloat32(off + 0, Number(v.x || 0), true);
            dv.setFloat32(off + 4, Number(v.y || 0), true);
            out[off + 8] = Number(v.r || 0) & 0xff;
            out[off + 9] = Number(v.g || 0) & 0xff;
            out[off + 10] = Number(v.b || 0) & 0xff;
            out[off + 11] = Number(v.a == null ? 255 : v.a) & 0xff;
            off += 12;
        }
        return out;
    }

    function __trueosSummarizeVerts(verts) {
        const list = Array.isArray(verts) ? verts : [];
        if (list.length <= 0) {
            return {
                minX: 0, maxX: 0, minY: 0, maxY: 0,
                minR: 0, maxR: 0, minG: 0, maxG: 0, minB: 0, maxB: 0,
                minA: 0, maxA: 0,
            };
        }
        let minX = Infinity;
        let maxX = -Infinity;
        let minY = Infinity;
        let maxY = -Infinity;
        let minR = 255;
        let maxR = 0;
        let minG = 255;
        let maxG = 0;
        let minB = 255;
        let maxB = 0;
        let minA = 255;
        let maxA = 0;
        for (let i = 0; i < list.length; i++) {
            const v = list[i] || {};
            const x = Number(v.x || 0);
            const y = Number(v.y || 0);
            const r = __trueosClampU8(v.r);
            const g = __trueosClampU8(v.g);
            const b = __trueosClampU8(v.b);
            const a = __trueosClampU8(v.a == null ? 255 : v.a);
            minX = Math.min(minX, x);
            maxX = Math.max(maxX, x);
            minY = Math.min(minY, y);
            maxY = Math.max(maxY, y);
            minR = Math.min(minR, r);
            maxR = Math.max(maxR, r);
            minG = Math.min(minG, g);
            maxG = Math.max(maxG, g);
            minB = Math.min(minB, b);
            maxB = Math.max(maxB, b);
            minA = Math.min(minA, a);
            maxA = Math.max(maxA, a);
        }
        return { minX, maxX, minY, maxY, minR, maxR, minG, maxG, minB, maxB, minA, maxA };
    }

    function __trueosClampU8(v) {
        const n = Number(v);
        if (!Number.isFinite(n)) return 0;
        if (n <= 0) return 0;
        if (n >= 255) return 255;
        return n | 0;
    }

    function __trueosHalfToFloat(h) {
        const x = Number(h) & 0xFFFF;
        const sign = (x & 0x8000) ? -1 : 1;
        const exp = (x >>> 10) & 0x1F;
        const mant = x & 0x03FF;
        if (exp === 0) {
            if (mant === 0) return sign * 0;
            return sign * Math.pow(2, -14) * (mant / 1024);
        }
        if (exp === 0x1F) {
            if (mant === 0) return sign * Infinity;
            return NaN;
        }
        return sign * Math.pow(2, exp - 15) * (1 + (mant / 1024));
    }

    function __trueosNormS8(v) {
        const n = Number(v) | 0;
        return Math.max(-1, Math.min(1, n / 127));
    }

    function __trueosNormU8(v) {
        const n = Number(v) | 0;
        return Math.max(0, Math.min(1, n / 255));
    }

    function __trueosNormS16(v) {
        const n = Number(v) | 0;
        return Math.max(-1, Math.min(1, n / 32767));
    }

    function __trueosNormU16(v) {
        const n = Number(v) | 0;
        return Math.max(0, Math.min(1, n / 65535));
    }

    function __trueosDecodeVertexLayout(pipeline) {
        const out = {
            stride: 12,
            posOffset: 0,
            posFormat: 'float32x2',
            colorOffset: 8,
            colorFormat: 'unorm8x4',
        };
        const desc = pipeline && pipeline.__desc;
        const v = desc && desc.vertex;
        const bufs = v && Array.isArray(v.buffers) ? v.buffers : null;
        const b0 = bufs && bufs.length > 0 ? bufs[0] : null;
        if (!b0 || typeof b0 !== 'object') return out;
        const stride = Number(b0.arrayStride || 0) | 0;
        if (stride > 0) out.stride = stride;
        const attrs = Array.isArray(b0.attributes) ? b0.attributes : [];
        for (let i = 0; i < attrs.length; i++) {
            const a = attrs[i] || {};
            const loc = Number(a.shaderLocation ?? -1) | 0;
            const off = Number(a.offset || 0) | 0;
            const fmt = String(a.format || '');
            if (loc === 0) {
                out.posOffset = Math.max(0, off);
                if (fmt) out.posFormat = fmt;
            } else if (loc === 1) {
                out.colorOffset = Math.max(0, off);
                if (fmt) out.colorFormat = fmt;
            }
        }
        return out;
    }

    function __trueosToNdcXY(x, y, viewportW, viewportH) {
        const vx = Number(x);
        const vy = Number(y);
        if (!Number.isFinite(vx) || !Number.isFinite(vy)) {
            return null;
        }
        // Some pipelines emit normalized viewport coords in [0..1] and rely on
        // shader math to expand to clip-space. Our replay path bypasses shaders,
        // so map that range explicitly into NDC.
        if (vx >= 0 && vx <= 1 && vy >= 0 && vy <= 1) {
            return {
                x: (vx * 2.0) - 1.0,
                y: 1.0 - (vy * 2.0),
            };
        }
        if (Math.abs(vx) <= 1.5 && Math.abs(vy) <= 1.5) {
            return { x: vx, y: vy };
        }
        const w = Math.max(1, Number(viewportW || 1));
        const h = Math.max(1, Number(viewportH || 1));
        const nx = ((vx / w) * 2.0) - 1.0;
        const ny = 1.0 - ((vy / h) * 2.0);
        return { x: nx, y: ny };
    }

    function __trueosReadVertexColor(dv, base, colorFormat, fallbackSeed) {
        const c = {
            // Bring-up plumbing mode: force very high-contrast, fully opaque
            // geometry colors so visibility does not depend on source material.
            r: (fallbackSeed & 1) ? 255 : 20,
            g: (fallbackSeed & 2) ? 255 : 20,
            b: (fallbackSeed & 4) ? 255 : 20,
            a: 255,
        };
        const fmt = String(colorFormat || '').toLowerCase();
        try {
            if (fmt === 'unorm8x4' || fmt === 'uint8x4' || fmt === 'snorm8x4' || fmt === 'sint8x4') {
                c.r = dv.getUint8(base + 0);
                c.g = dv.getUint8(base + 1);
                c.b = dv.getUint8(base + 2);
                c.a = dv.getUint8(base + 3);
            } else if (fmt === 'float32x4') {
                c.r = __trueosClampU8(dv.getFloat32(base + 0, true) * 255);
                c.g = __trueosClampU8(dv.getFloat32(base + 4, true) * 255);
                c.b = __trueosClampU8(dv.getFloat32(base + 8, true) * 255);
                c.a = __trueosClampU8(dv.getFloat32(base + 12, true) * 255);
            }
        } catch (_) {
            // Keep fallback debug color when color decode reads out of range.
        }
        // For geometry confirmation, avoid near-white-on-white and low-alpha output.
        if (c.a < 255) {
            c.a = 255;
        }
        if (c.r > 235 && c.g > 235 && c.b > 235) {
            c.r = 20;
            c.g = 255;
            c.b = 20;
        }
        c.r = __trueosClampU8(c.r);
        c.g = __trueosClampU8(c.g);
        c.b = __trueosClampU8(c.b);
        c.a = __trueosClampU8(c.a);
        return c;
    }

    function __trueosReadVertexPos(dv, base, posFormat) {
        const fmt = String(posFormat || '').toLowerCase();
        if (fmt === 'float16x2' || fmt === 'float16x3' || fmt === 'float16x4') {
            return {
                x: __trueosHalfToFloat(dv.getUint16(base + 0, true)),
                y: __trueosHalfToFloat(dv.getUint16(base + 2, true)),
            };
        }
        if (fmt === 'snorm16x2' || fmt === 'snorm16x4') {
            return {
                x: __trueosNormS16(dv.getInt16(base + 0, true)),
                y: __trueosNormS16(dv.getInt16(base + 2, true)),
            };
        }
        if (fmt === 'unorm16x2' || fmt === 'unorm16x4') {
            return {
                x: __trueosNormU16(dv.getUint16(base + 0, true)),
                y: __trueosNormU16(dv.getUint16(base + 2, true)),
            };
        }
        if (fmt === 'sint16x2' || fmt === 'sint16x4') {
            return {
                x: dv.getInt16(base + 0, true),
                y: dv.getInt16(base + 2, true),
            };
        }
        if (fmt === 'uint16x2' || fmt === 'uint16x4') {
            return {
                x: dv.getUint16(base + 0, true),
                y: dv.getUint16(base + 2, true),
            };
        }
        if (fmt === 'snorm8x2' || fmt === 'snorm8x4') {
            return {
                x: __trueosNormS8(dv.getInt8(base + 0)),
                y: __trueosNormS8(dv.getInt8(base + 1)),
            };
        }
        if (fmt === 'unorm8x2' || fmt === 'unorm8x4') {
            return {
                x: __trueosNormU8(dv.getUint8(base + 0)),
                y: __trueosNormU8(dv.getUint8(base + 1)),
            };
        }
        if (fmt === 'sint8x2' || fmt === 'sint8x4') {
            return {
                x: dv.getInt8(base + 0),
                y: dv.getInt8(base + 1),
            };
        }
        if (fmt === 'uint8x2' || fmt === 'uint8x4') {
            return {
                x: dv.getUint8(base + 0),
                y: dv.getUint8(base + 1),
            };
        }
        if (fmt === 'sint32x2' || fmt === 'sint32x3' || fmt === 'sint32x4') {
            return {
                x: dv.getInt32(base + 0, true),
                y: dv.getInt32(base + 4, true),
            };
        }
        if (fmt === 'uint32x2' || fmt === 'uint32x3' || fmt === 'uint32x4') {
            return {
                x: dv.getUint32(base + 0, true),
                y: dv.getUint32(base + 4, true),
            };
        }
        if (fmt === 'float32x2' || fmt === 'float32x3' || fmt === 'float32x4') {
            return {
                x: dv.getFloat32(base + 0, true),
                y: dv.getFloat32(base + 4, true),
            };
        }
        return {
            x: dv.getFloat32(base + 0, true),
            y: dv.getFloat32(base + 4, true),
        };
    }

    function __trueosReadIndex(idxData, idxFormat, idxOffset, indexPos) {
        const base = Math.max(0, Number(idxOffset || 0) | 0);
        const i = Math.max(0, Number(indexPos || 0) | 0);
        const fmt = String(idxFormat || 'uint16').toLowerCase();
        if (fmt === 'uint32') {
            const off = base + (i * 4);
            if ((off + 4) > idxData.byteLength) return -1;
            const dv = new DataView(idxData.buffer, idxData.byteOffset, idxData.byteLength);
            return dv.getUint32(off, true);
        }
        const off = base + (i * 2);
        if ((off + 2) > idxData.byteLength) return -1;
        const dv = new DataView(idxData.buffer, idxData.byteOffset, idxData.byteLength);
        return dv.getUint16(off, true);
    }

    function __trueosEmitVertexFromBuffer(verts, vtxData, vtxByteOffset, layout, viewportW, viewportH, fallbackSeed, stats) {
        if (!(vtxData instanceof Uint8Array)) return false;
        const stride = Math.max(4, Number(layout?.stride || 12) | 0);
        const posOffset = Math.max(0, Number(layout?.posOffset || 0) | 0);
        const colorOffset = Math.max(0, Number(layout?.colorOffset || 8) | 0);
        if ((vtxByteOffset + posOffset + 8) > vtxData.byteLength) {
            if (stats) stats.oobVertexDrops = (Number(stats.oobVertexDrops || 0) | 0) + 1;
            return false;
        }
        const dv = new DataView(vtxData.buffer, vtxData.byteOffset, vtxData.byteLength);
        let pos;
        try {
            pos = __trueosReadVertexPos(dv, vtxByteOffset + posOffset, layout?.posFormat);
        } catch (_) {
            if (stats) stats.decodeVertexDrops = (Number(stats.decodeVertexDrops || 0) | 0) + 1;
            return false;
        }
        let ndc = __trueosToNdcXY(pos.x, pos.y, viewportW, viewportH);
        if (!ndc) {
            // If declared format decode yields non-finite values, try float32x2 bytes
            // as a pragmatic fallback for mismatched pipeline descriptors.
            try {
                const alt = {
                    x: dv.getFloat32(vtxByteOffset + posOffset + 0, true),
                    y: dv.getFloat32(vtxByteOffset + posOffset + 4, true),
                };
                ndc = __trueosToNdcXY(alt.x, alt.y, viewportW, viewportH);
            } catch (_) {
                ndc = null;
            }
        }
        if (!ndc) {
            if (stats) stats.nonFinitePosDrops = (Number(stats.nonFinitePosDrops || 0) | 0) + 1;
            return false;
        }
        let color;
        if ((vtxByteOffset + colorOffset + 4) <= vtxData.byteLength) {
            color = __trueosReadVertexColor(dv, vtxByteOffset + colorOffset, layout?.colorFormat, fallbackSeed);
        } else {
            color = __trueosReadVertexColor(dv, 0, '', fallbackSeed);
        }
        verts.push({
            x: ndc.x,
            y: ndc.y,
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
        });
        return true;
    }

    function __trueosReplaySubmitToCmdStream(cmdBuffers) {
        const cmd = G.__trueosCmdStream;
        if (!cmd
            || typeof cmd.beginFrame !== 'function'
            || typeof cmd.endFrame !== 'function'
            || typeof cmd.drawTrianglesU8 !== 'function') {
            return { replayed: false, draws: 0, passes: 0, ops: 0 };
        }

        const list = Array.isArray(cmdBuffers)
            ? cmdBuffers
            : (cmdBuffers == null ? [] : [cmdBuffers]);
        let draws = 0;
        let passes = 0;
        let ops = 0;
        let viewportW = 1280;
        let viewportH = 800;
        const verts = [];
        const stats = {
            droppedTris: 0,
            nonFinitePosDrops: 0,
            decodeVertexDrops: 0,
            oobVertexDrops: 0,
        };

        for (let i = 0; i < list.length; i++) {
            const cb = list[i];
            const cmds = Array.isArray(cb && cb.__cmds) ? cb.__cmds : [];
            for (let c = 0; c < cmds.length; c++) {
                const cmdRec = cmds[c];
                if (cmdRec && cmdRec.__kind === 'render-pass') {
                    passes++;
                    const passOps = Array.isArray(cmdRec.__ops) ? cmdRec.__ops : [];
                    let currentPipeline = null;
                    let vtxBuf = null;
                    let vtxOff = 0;
                    let idxBuf = null;
                    let idxFmt = 'uint16';
                    let idxOff = 0;
                    for (let p = 0; p < passOps.length; p++) {
                        const op = passOps[p];
                        if (!Array.isArray(op) || op.length <= 0)
                            continue;
                        ops++;
                        const name = String(op[0] || '');
                        if (name === 'setPipeline') {
                            currentPipeline = op[1] || null;
                        }
                        else if (name === 'setVertexBuffer') {
                            const slot = Number(op[1] || 0) | 0;
                            if (slot === 0) {
                                vtxBuf = op[2] || null;
                                vtxOff = Math.max(0, Number(op[3] || 0) | 0);
                            }
                        }
                        else if (name === 'setIndexBuffer') {
                            idxBuf = op[1] || null;
                            idxFmt = String(op[2] || 'uint16');
                            idxOff = Math.max(0, Number(op[3] || 0) | 0);
                        }
                        else if (name === 'setViewport') {
                            const w = Number(op[3] || 0);
                            const h = Number(op[4] || 0);
                            if (Number.isFinite(w) && Number.isFinite(h) && w > 0 && h > 0) {
                                viewportW = Math.max(1, w | 0);
                                viewportH = Math.max(1, h | 0);
                            }
                        }
                        else if (name === 'draw') {
                            const vtxCount = Math.max(0, Number(op[1] || 0) | 0);
                            const firstV = Math.max(0, Number(op[3] || 0) | 0);
                            const vbData = vtxBuf && (vtxBuf.__data instanceof Uint8Array) ? vtxBuf.__data : null;
                            const layout = __trueosDecodeVertexLayout(currentPipeline);
                            if (vbData) {
                                const stride = Math.max(4, Number(layout.stride || 12) | 0);
                                const triCount = (vtxCount / 3) | 0;
                                for (let t = 0; t < triCount; t++) {
                                    const i0 = firstV + (t * 3);
                                    const i1 = i0 + 1;
                                    const i2 = i0 + 2;
                                    const b0 = vtxOff + (i0 * stride);
                                    const b1 = vtxOff + (i1 * stride);
                                    const b2 = vtxOff + (i2 * stride);
                                    const ok0 = __trueosEmitVertexFromBuffer(verts, vbData, b0, layout, viewportW, viewportH, draws + t + 1, stats);
                                    const ok1 = __trueosEmitVertexFromBuffer(verts, vbData, b1, layout, viewportW, viewportH, draws + t + 2, stats);
                                    const ok2 = __trueosEmitVertexFromBuffer(verts, vbData, b2, layout, viewportW, viewportH, draws + t + 3, stats);
                                    if (!(ok0 && ok1 && ok2)) {
                                        if (ok0) verts.pop();
                                        if (ok1) verts.pop();
                                        if (ok2) verts.pop();
                                        stats.droppedTris = (Number(stats.droppedTris || 0) | 0) + 1;
                                        break;
                                    }
                                }
                            }
                            draws++;
                        }
                        else if (name === 'drawIndexed') {
                            const idxCount = Math.max(0, Number(op[1] || 0) | 0);
                            const firstIndex = Math.max(0, Number(op[3] || 0) | 0);
                            const baseVertex = Number(op[4] || 0) | 0;
                            const vbData = vtxBuf && (vtxBuf.__data instanceof Uint8Array) ? vtxBuf.__data : null;
                            const ibData = idxBuf && (idxBuf.__data instanceof Uint8Array) ? idxBuf.__data : null;
                            const layout = __trueosDecodeVertexLayout(currentPipeline);
                            if (vbData && ibData) {
                                const stride = Math.max(4, Number(layout.stride || 12) | 0);
                                const triCount = (idxCount / 3) | 0;
                                for (let t = 0; t < triCount; t++) {
                                    const iBase = firstIndex + (t * 3);
                                    const idx0 = __trueosReadIndex(ibData, idxFmt, idxOff, iBase + 0);
                                    const idx1 = __trueosReadIndex(ibData, idxFmt, idxOff, iBase + 1);
                                    const idx2 = __trueosReadIndex(ibData, idxFmt, idxOff, iBase + 2);
                                    if (idx0 < 0 || idx1 < 0 || idx2 < 0) break;
                                    const v0 = idx0 + baseVertex;
                                    const v1 = idx1 + baseVertex;
                                    const v2 = idx2 + baseVertex;
                                    if (v0 < 0 || v1 < 0 || v2 < 0) break;
                                    const b0 = vtxOff + (v0 * stride);
                                    const b1 = vtxOff + (v1 * stride);
                                    const b2 = vtxOff + (v2 * stride);
                                    const ok0 = __trueosEmitVertexFromBuffer(verts, vbData, b0, layout, viewportW, viewportH, draws + t + 1, stats);
                                    const ok1 = __trueosEmitVertexFromBuffer(verts, vbData, b1, layout, viewportW, viewportH, draws + t + 2, stats);
                                    const ok2 = __trueosEmitVertexFromBuffer(verts, vbData, b2, layout, viewportW, viewportH, draws + t + 3, stats);
                                    if (!(ok0 && ok1 && ok2)) {
                                        if (ok0) verts.pop();
                                        if (ok1) verts.pop();
                                        if (ok2) verts.pop();
                                        stats.droppedTris = (Number(stats.droppedTris || 0) | 0) + 1;
                                        break;
                                    }
                                }
                            }
                            draws++;
                        }
                    }
                }
                else if (Array.isArray(cmdRec)) {
                    ops++;
                    const opName = String(cmdRec[0] || '');
                    if (opName === 'copyBufferToTexture') {
                        const src = cmdRec[1] || null;
                        const dst = cmdRec[2] || null;
                        const size = cmdRec[3] || null;
                        const srcBuf = src && src.buffer;
                        const srcData = srcBuf && (srcBuf.__data instanceof Uint8Array) ? srcBuf.__data : null;
                        const dstTex = dst && dst.texture;
                        if (!(srcData instanceof Uint8Array) || !dstTex) {
                            continue;
                        }
                        const w = Math.max(0, Number((size && (size.width ?? size[0])) || 0) | 0);
                        const h = Math.max(0, Number((size && (size.height ?? size[1])) || 0) | 0);
                        if (w <= 0 || h <= 0) {
                            continue;
                        }
                        const srcOff = Math.max(0, Number((src && src.offset) || 0) | 0);
                        const bpr = Math.max(w * 4, Number((src && src.bytesPerRow) || (w * 4)) | 0);
                        const need = srcOff + (Math.max(0, h - 1) * bpr) + (w * 4);
                        if (need > srcData.byteLength) {
                            continue;
                        }
                        const ox = Math.max(0, Number(dst && dst.origin && dst.origin.x || 0) | 0);
                        const oy = Math.max(0, Number(dst && dst.origin && dst.origin.y || 0) | 0);
                        const srcSlice = srcData.subarray(srcOff, need);
                        __trueosUploadLinearRgbaToTexture(dstTex, srcSlice, bpr, w, h, ox, oy);
                        continue;
                    }
                    if (opName === 'copyTextureToTexture') {
                        const src = cmdRec[1] || null;
                        const dst = cmdRec[2] || null;
                        const size = cmdRec[3] || null;
                        const srcTex = src && src.texture;
                        const dstTex = dst && dst.texture;
                        const srcBytes = srcTex && srcTex.__rgbas && (srcTex.__rgbas.rgba instanceof Uint8Array)
                            ? srcTex.__rgbas.rgba
                            : null;
                        if (!(srcBytes instanceof Uint8Array) || !dstTex) {
                            continue;
                        }
                        const w = Math.max(0, Number((size && (size.width ?? size[0])) || srcTex.width || 0) | 0);
                        const h = Math.max(0, Number((size && (size.height ?? size[1])) || srcTex.height || 0) | 0);
                        if (w <= 0 || h <= 0) {
                            continue;
                        }
                        const sx = Math.max(0, Number(src && src.origin && src.origin.x || 0) | 0);
                        const sy = Math.max(0, Number(src && src.origin && src.origin.y || 0) | 0);
                        const ox = Math.max(0, Number(dst && dst.origin && dst.origin.x || 0) | 0);
                        const oy = Math.max(0, Number(dst && dst.origin && dst.origin.y || 0) | 0);
                        const srcW = Math.max(1, Number(srcTex.width || 1) | 0);
                        const rowBytes = w * 4;
                        const packed = new Uint8Array(rowBytes * h);
                        for (let yy = 0; yy < h; yy++) {
                            const srcRow = (((sy + yy) * srcW) + sx) * 4;
                            const dstRow = yy * rowBytes;
                            const end = srcRow + rowBytes;
                            if (end > srcBytes.byteLength) break;
                            packed.set(srcBytes.subarray(srcRow, end), dstRow);
                        }
                        __trueosUploadLinearRgbaToTexture(dstTex, packed, rowBytes, w, h, ox, oy);
                        continue;
                    }
                }
            }
        }

        if (verts.length <= 0) {
            return {
                replayed: true,
                draws,
                passes,
                ops,
                flushed: false,
                vertices: 0,
                droppedTris: Number(stats.droppedTris || 0) | 0,
                nonFinitePosDrops: Number(stats.nonFinitePosDrops || 0) | 0,
                decodeVertexDrops: Number(stats.decodeVertexDrops || 0) | 0,
                oobVertexDrops: Number(stats.oobVertexDrops || 0) | 0,
                vis: __trueosSummarizeVerts(verts),
            };
        }

        if (typeof cmd.setViewport === 'function') {
            cmd.setViewport(viewportW, viewportH);
        }
        if (typeof cmd.setClearRgb === 'function') {
            cmd.setClearRgb(0xFFFFFF);
        }
        cmd.beginFrame();
        const bytes = __trueosPackVerts12(verts);
        if (bytes && bytes.byteLength > 0) {
            cmd.drawTrianglesU8(bytes);
        }
        cmd.endFrame();

        return {
            replayed: true,
            draws,
            passes,
            ops,
            flushed: true,
            vertices: verts.length,
            droppedTris: Number(stats.droppedTris || 0) | 0,
            nonFinitePosDrops: Number(stats.nonFinitePosDrops || 0) | 0,
            decodeVertexDrops: Number(stats.decodeVertexDrops || 0) | 0,
            oobVertexDrops: Number(stats.oobVertexDrops || 0) | 0,
            vis: __trueosSummarizeVerts(verts),
        };
    }

    function __trueosResolveQueueWaiters() {
        const done = Number(__trueosWebGpuState.completedFence || 0) | 0;
        const waiters = Array.isArray(__trueosWebGpuState.waiters) ? __trueosWebGpuState.waiters : [];
        if (waiters.length <= 0) return;
        const keep = [];
        for (let i = 0; i < waiters.length; i++) {
            const w = waiters[i] || null;
            const fence = Number(w && w.fence || 0) | 0;
            if (fence <= done) {
                try {
                    if (typeof w.resolve === 'function') w.resolve();
                } catch (_) {
                    // Keep queue progress independent from waiter handlers.
                }
            } else {
                keep.push(w);
            }
        }
        __trueosWebGpuState.waiters = keep;
    }

    function __trueosScheduleSubmitFlush() {
        if (__trueosWebGpuState.flushScheduled) return;
        __trueosWebGpuState.flushScheduled = true;
        const run = () => {
            __trueosWebGpuState.flushScheduled = false;
            __trueosFlushSubmitQueue('raf');
        };
        if (typeof G.requestAnimationFrame === 'function') {
            G.requestAnimationFrame(() => run());
        } else {
            Promise.resolve().then(run);
        }
    }

    function __trueosFlushSubmitQueue(reason) {
        if (__trueosWebGpuState.flushing) return;
        const queue = Array.isArray(__trueosWebGpuState.submitQueue) ? __trueosWebGpuState.submitQueue : [];
        if (queue.length <= 0) {
            __trueosResolveQueueWaiters();
            return;
        }
        __trueosWebGpuState.flushing = true;
        try {
            const batch = queue.splice(0, queue.length);
            const mergedCmdBuffers = [];
            let totalPasses = 0;
            let totalOps = 0;
            let totalDraws = 0;
            let maxFence = Number(__trueosWebGpuState.completedFence || 0) | 0;
            for (let i = 0; i < batch.length; i++) {
                const it = batch[i] || {};
                const cbs = Array.isArray(it.cmdBuffers) ? it.cmdBuffers : [];
                for (let j = 0; j < cbs.length; j++) {
                    const cb = cbs[j];
                    if (cb) mergedCmdBuffers.push(cb);
                }
                totalPasses += Number(it.passCount || 0) | 0;
                totalOps += Number(it.opCount || 0) | 0;
                totalDraws += Number(it.drawCount || 0) | 0;
                const f = Number(it.fence || 0) | 0;
                if (f > maxFence) maxFence = f;
            }
            const replay = __trueosReplaySubmitToCmdStream(mergedCmdBuffers);
            __trueosWebGpuState.completedFence = maxFence;
            __trueosResolveQueueWaiters();
            const flushNo = (Number(__trueosWebGpuState.flushCount || 0) | 0) + 1;
            __trueosWebGpuState.flushCount = flushNo;
            const shouldLog = flushNo <= 8
                || (flushNo % 60) === 0
                || totalDraws > 0
                || Number(replay.vertices || 0) > 0;
            if (shouldLog) {
                const vis = replay && replay.vis ? replay.vis : null;
                const ndcX = vis ? `${Number(vis.minX || 0).toFixed(3)}..${Number(vis.maxX || 0).toFixed(3)}` : '0.000..0.000';
                const ndcY = vis ? `${Number(vis.minY || 0).toFixed(3)}..${Number(vis.maxY || 0).toFixed(3)}` : '0.000..0.000';
                const rgb = vis
                    ? `r${Number(vis.minR || 0) | 0}-${Number(vis.maxR || 0) | 0}/g${Number(vis.minG || 0) | 0}-${Number(vis.maxG || 0) | 0}/b${Number(vis.minB || 0) | 0}-${Number(vis.maxB || 0) | 0}`
                    : 'r0-0/g0-0/b0-0';
                const a = vis ? `${Number(vis.minA || 0) | 0}-${Number(vis.maxA || 0) | 0}` : '0-0';
                console.log(`[webgpu-bridge] queue.flush #${flushNo} reason=${String(reason || 'unknown')} submits=${batch.length} cmdBuffers=${mergedCmdBuffers.length} passes=${totalPasses} ops=${totalOps} draws=${totalDraws} replay=${replay.replayed ? 1 : 0} replayDraws=${Number(replay.draws || 0) | 0} verts=${Number(replay.vertices || 0) | 0} droppedTris=${Number(replay.droppedTris || 0) | 0} nonFinitePosDrops=${Number(replay.nonFinitePosDrops || 0) | 0} decodeVertexDrops=${Number(replay.decodeVertexDrops || 0) | 0} oobVertexDrops=${Number(replay.oobVertexDrops || 0) | 0} ndc=${ndcX},${ndcY} rgb=${rgb} a=${a} presented=${replay.flushed ? 1 : 0} doneFence=${Number(__trueosWebGpuState.completedFence || 0) | 0}`);
            }
        } finally {
            __trueosWebGpuState.flushing = false;
        }
    }

    G.__trueosMakeGpuQueue = function __trueosMakeGpuQueue(_device) {
        if (!Array.isArray(__trueosWebGpuState.submitQueue)) __trueosWebGpuState.submitQueue = [];
        if (!Array.isArray(__trueosWebGpuState.waiters)) __trueosWebGpuState.waiters = [];
        if (typeof __trueosWebGpuState.maxSubmitQueue !== 'number') __trueosWebGpuState.maxSubmitQueue = 8;
        if (typeof __trueosWebGpuState.nextFence !== 'number') __trueosWebGpuState.nextFence = 1;
        if (typeof __trueosWebGpuState.completedFence !== 'number') __trueosWebGpuState.completedFence = 0;
        if (typeof __trueosWebGpuState.flushCount !== 'number') __trueosWebGpuState.flushCount = 0;
        if (typeof __trueosWebGpuState.flushScheduled !== 'boolean') __trueosWebGpuState.flushScheduled = false;
        if (typeof __trueosWebGpuState.flushing !== 'boolean') __trueosWebGpuState.flushing = false;
        if (typeof __trueosWebGpuState.lastEnqueuedFence !== 'number') __trueosWebGpuState.lastEnqueuedFence = 0;
        return {
            __id: __trueosWebGpuState.nextId++,
            submit(_cmdBuffers) {
                const cmdBuffers = Array.isArray(_cmdBuffers)
                    ? _cmdBuffers
                    : (_cmdBuffers == null ? [] : [_cmdBuffers]);
                let passCount = 0;
                let opCount = 0;
                let drawCount = 0;
                let cmdBufferCount = 0;
                for (let i = 0; i < cmdBuffers.length; i++) {
                    const cb = cmdBuffers[i];
                    if (!cb)
                        continue;
                    cmdBufferCount++;
                    const sum = (cb && typeof cb === 'object' && cb.__summary && typeof cb.__summary === 'object')
                        ? cb.__summary
                        : { passes: 0, ops: 0, draws: 0 };
                    passCount += Number(sum.passes || 0) | 0;
                    opCount += Number(sum.ops || 0) | 0;
                    drawCount += Number(sum.draws || 0) | 0;
                }
                __trueosWebGpuState.submitCount = (Number(__trueosWebGpuState.submitCount || 0) | 0) + 1;
                __trueosWebGpuState.submittedCmdBuffers = (Number(__trueosWebGpuState.submittedCmdBuffers || 0) | 0) + cmdBufferCount;
                __trueosWebGpuState.submittedPasses = (Number(__trueosWebGpuState.submittedPasses || 0) | 0) + passCount;
                __trueosWebGpuState.submittedOps = (Number(__trueosWebGpuState.submittedOps || 0) | 0) + opCount;
                __trueosWebGpuState.submittedDraws = (Number(__trueosWebGpuState.submittedDraws || 0) | 0) + drawCount;
                const submitNo = Number(__trueosWebGpuState.submitCount || 0) | 0;
                const fence = Number(__trueosWebGpuState.nextFence || 1) | 0;
                __trueosWebGpuState.nextFence = fence + 1;
                __trueosWebGpuState.lastEnqueuedFence = fence;
                const queue = __trueosWebGpuState.submitQueue;
                const maxQueue = Math.max(1, Number(__trueosWebGpuState.maxSubmitQueue || 8) | 0);
                if (queue.length >= maxQueue) {
                    __trueosFlushSubmitQueue('backpressure');
                }
                queue.push({
                    fence,
                    cmdBuffers,
                    cmdBufferCount,
                    passCount,
                    opCount,
                    drawCount,
                    submitNo,
                });
                __trueosScheduleSubmitFlush();
                const shouldLog = submitNo <= 8 || (submitNo % 60) === 0 || drawCount > 0 || queue.length >= maxQueue;
                if (shouldLog) {
                    console.log(`[webgpu-bridge] queue.submit #${submitNo} cmdBuffers=${cmdBufferCount} passes=${passCount} ops=${opCount} draws=${drawCount} totalDraws=${Number(__trueosWebGpuState.submittedDraws || 0) | 0} queued=${queue.length} maxQueue=${maxQueue} fence=${fence}`);
                }
            },
            writeBuffer(buffer, bufferOffset, data, dataOffset = 0, size) {
                if (!buffer || !(buffer.__data instanceof Uint8Array)) return;
                const srcAll = (typeof G.__trueosToU8 === 'function') ? G.__trueosToU8(data) : new Uint8Array();
                if (!(srcAll instanceof Uint8Array)) return;
                const bo = Math.max(0, Number(bufferOffset || 0) | 0);
                const so = Math.max(0, Number(dataOffset || 0) | 0);
                const n = (size == null) ? Math.max(0, srcAll.byteLength - so) : Math.max(0, Number(size) | 0);
                const end = Math.min(buffer.__data.byteLength, bo + n);
                const srcEnd = Math.min(srcAll.byteLength, so + (end - bo));
                if (end > bo && srcEnd > so) {
                    buffer.__data.set(srcAll.subarray(so, srcEnd), bo);
                }
            },
            copyExternalImageToTexture(source, destination, copySize) {
                const tex = destination && destination.texture;
                if (!tex) return;

                const srcObj = source && source.source ? source.source : source;
                if (!srcObj) return;

                const sizeObj = copySize || {};
                const w = Math.max(
                    0,
                    Number(
                        sizeObj.width
                        ?? (Array.isArray(sizeObj) ? sizeObj[0] : undefined)
                        ?? srcObj.width
                        ?? srcObj.videoWidth
                        ?? srcObj.naturalWidth
                        ?? 0
                    ) | 0
                );
                const h = Math.max(
                    0,
                    Number(
                        sizeObj.height
                        ?? (Array.isArray(sizeObj) ? sizeObj[1] : undefined)
                        ?? srcObj.height
                        ?? srcObj.videoHeight
                        ?? srcObj.naturalHeight
                        ?? 0
                    ) | 0
                );
                if (w <= 0 || h <= 0) return;

                const origin = destination && destination.origin ? destination.origin : null;
                const ox = Math.max(0, Number(origin && origin.x ? origin.x : 0) | 0);
                const oy = Math.max(0, Number(origin && origin.y ? origin.y : 0) | 0);

                // Fallback path: accept plain RGBA-backed objects and copy row-wise.
                const srcBytes = (srcObj.__rgbas && srcObj.__rgbas.rgba instanceof Uint8Array)
                    ? srcObj.__rgbas.rgba
                    : (srcObj.__data instanceof Uint8Array)
                        ? srcObj.__data
                        : (srcObj.data instanceof Uint8Array)
                            ? srcObj.data
                            : (srcObj.data instanceof Uint8ClampedArray)
                                ? new Uint8Array(srcObj.data.buffer, srcObj.data.byteOffset, srcObj.data.byteLength)
                                : null;

                if (!(srcBytes instanceof Uint8Array)) return;

                const bpr = Math.max(w * 4, Math.min(srcBytes.byteLength, ((srcObj.width || w) | 0) * 4));
                if (typeof G.__trueosUploadLinearRgbaToTexture === 'function') {
                    G.__trueosUploadLinearRgbaToTexture(tex, srcBytes, bpr, w, h, ox, oy);
                }
            },
            writeTexture(destination, data, dataLayout = {}, size = {}) {
                const tex = destination && destination.texture;
                if (!tex) return;
                const srcAll = (typeof G.__trueosToU8 === 'function') ? G.__trueosToU8(data) : new Uint8Array();
                if (!(srcAll instanceof Uint8Array)) return;
                const w = Math.max(0, Number((size && (size.width ?? size[0])) || tex.width || 0) | 0);
                const h = Math.max(0, Number((size && (size.height ?? size[1])) || tex.height || 0) | 0);
                if (w <= 0 || h <= 0) return;
                const srcOff = Math.max(0, Number(dataLayout && dataLayout.offset || 0) | 0);
                const bpr = Math.max(w * 4, Number(dataLayout && dataLayout.bytesPerRow || (w * 4)) | 0);
                const need = srcOff + (Math.max(0, h - 1) * bpr) + (w * 4);
                if (need > srcAll.byteLength) return;
                const ox = Math.max(0, Number(destination && destination.origin && destination.origin.x || 0) | 0);
                const oy = Math.max(0, Number(destination && destination.origin && destination.origin.y || 0) | 0);
                const src = srcAll.subarray(srcOff, need);
                __trueosUploadLinearRgbaToTexture(tex, src, bpr, w, h, ox, oy);
            },
            async onSubmittedWorkDone() {
                const target = Number(__trueosWebGpuState.lastEnqueuedFence || __trueosWebGpuState.completedFence || 0) | 0;
                if ((Number(__trueosWebGpuState.completedFence || 0) | 0) >= target) {
                    return;
                }
                return new Promise((resolve) => {
                    const waiters = Array.isArray(__trueosWebGpuState.waiters) ? __trueosWebGpuState.waiters : [];
                    waiters.push({ fence: target, resolve });
                    __trueosWebGpuState.waiters = waiters;
                    __trueosScheduleSubmitFlush();
                });
            },
        };
    };
}

function __trueosMakeGpuDevice(adapter, desc = {}) {
    const device = {
        __id: __trueosWebGpuState.nextId++,
        label: String(desc.label || ''),
        lost: Promise.resolve({ reason: 'destroyed', message: '' }),
        features: new Set(desc.requiredFeatures || []),
        limits: Object.assign({}, __trueosGpuLimits),
        queue: null,
        createBuffer(d = {}) { return __trueosMakeGpuBuffer(d); },
        createTexture(d = {}) {
            const size = d.size || {};
            const w = Number(size.width || size[0] || 1) | 0;
            const h = Number(size.height || size[1] || 1) | 0;
            return __trueosMakeGpuTexture(d.label, w, h, d.format || __trueosWebGpuState.preferredCanvasFormat, d.usage);
        },
        createSampler(d = {}) { return { __id: __trueosWebGpuState.nextId++, label: String(d.label || ''), __desc: d }; },
        createShaderModule(d = {}) { return { __id: __trueosWebGpuState.nextId++, label: String(d.label || ''), code: d.code || '' }; },
        createBindGroupLayout(d = {}) { return { __id: __trueosWebGpuState.nextId++, label: String(d.label || ''), entries: d.entries || [] }; },
        createPipelineLayout(d = {}) { return { __id: __trueosWebGpuState.nextId++, label: String(d.label || ''), bindGroupLayouts: d.bindGroupLayouts || [] }; },
        createBindGroup(d = {}) { return { __id: __trueosWebGpuState.nextId++, label: String(d.label || ''), layout: d.layout, entries: d.entries || [] }; },
        createRenderPipeline(d = {}) {
            const p = { __id: __trueosWebGpuState.nextId++, label: String(d.label || ''), __desc: d };
            p.getBindGroupLayout = (i) => (d.layout && d.layout.bindGroupLayouts && d.layout.bindGroupLayouts[i]) || { __id: __trueosWebGpuState.nextId++, entries: [] };
            return p;
        },
        createCommandEncoder(d = {}) { return G.__trueosMakeGpuCommandEncoder(device, d); },
        createRenderBundleEncoder(d = {}) {
            const ops = [];
            return {
                __id: __trueosWebGpuState.nextId++,
                __desc: d,
                __ops: ops,
                setPipeline(p) { ops.push(['setPipeline', p]); },
                setBindGroup(i, bg) { ops.push(['setBindGroup', i, bg]); },
                setVertexBuffer(slot, b, off = 0, size) { ops.push(['setVertexBuffer', slot, b, off, size]); },
                setIndexBuffer(b, f = 'uint16', off = 0, size) { ops.push(['setIndexBuffer', b, f, off, size]); },
                setViewport(x, y, w, h, minD = 0, maxD = 1) { ops.push(['setViewport', x, y, w, h, minD, maxD]); },
                setScissorRect(x, y, w, h) { ops.push(['setScissorRect', x, y, w, h]); },
                setStencilReference(v) { ops.push(['setStencilReference', v]); },
                setBlendConstant(c) { ops.push(['setBlendConstant', c]); },
                draw(vtxCount, instCount = 1, firstV = 0, firstI = 0) { ops.push(['draw', vtxCount, instCount, firstV, firstI]); },
                drawIndexed(idxCount, instCount = 1, firstIndex = 0, baseVertex = 0, firstInstance = 0) {
                    ops.push(['drawIndexed', idxCount, instCount, firstIndex, baseVertex, firstInstance]);
                },
                finish() {
                    return {
                        __id: __trueosWebGpuState.nextId++,
                        __bundle: true,
                        __desc: d,
                        __ops: ops.slice(),
                    };
                },
            };
        },
        createCommandBuffer() { return { __id: __trueosWebGpuState.nextId++ }; },
        pushErrorScope() {},
        async popErrorScope() { return null; },
        destroy() {},
    };
    device.queue = G.__trueosMakeGpuQueue(device);
    return device;
}

function __trueosMakeGpuAdapter(desc = {}) {
    return {
        __id: __trueosWebGpuState.nextId++,
        isFallbackAdapter: !!desc.forceFallbackAdapter,
        features: new Set(),
        limits: Object.assign({}, __trueosGpuLimits),
        info: {
            vendor: 'TRUEOS',
            architecture: 'virtio-gpu',
            device: __trueosWebGpuState.backendName,
            description: 'TRUEOS WebGPU bridge over command stream',
        },
        async requestAdapterInfo() {
            return this.info;
        },
        async requestDevice(deviceDesc = {}) {
            return __trueosMakeGpuDevice(this, deviceDesc || {});
        },
    };
}

function __trueosMakeGpuCanvasContext(canvas) {
    const state = {
        configured: false,
        format: __trueosWebGpuState.preferredCanvasFormat,
        device: null,
        alphaMode: 'opaque',
        usage: null,
        width: Math.max(1, Number(canvas.width || 1) | 0),
        height: Math.max(1, Number(canvas.height || 1) | 0),
    };
    return {
        __id: __trueosWebGpuState.nextId++,
        canvas,
        configure(cfg = {}) {
            state.configured = true;
            state.device = cfg.device || null;
            state.format = String(cfg.format || __trueosWebGpuState.preferredCanvasFormat);
            state.alphaMode = String(cfg.alphaMode || 'opaque');
            state.usage = (cfg.usage == null) ? null : Number(cfg.usage);
            state.width = Math.max(1, Number(canvas.width || 1) | 0);
            state.height = Math.max(1, Number(canvas.height || 1) | 0);
        },
        unconfigure() {
            state.configured = false;
            state.device = null;
        },
        getCurrentTexture() {
            state.width = Math.max(1, Number(canvas.width || 1) | 0);
            state.height = Math.max(1, Number(canvas.height || 1) | 0);
            return __trueosMakeGpuTexture(
                'canvas-current-texture',
                state.width,
                state.height,
                state.format,
                state.usage == null
                    ? ((G.GPUTextureUsage && (G.GPUTextureUsage.RENDER_ATTACHMENT | G.GPUTextureUsage.COPY_SRC | G.GPUTextureUsage.COPY_DST)) || 0x13)
                    : state.usage
            );
        },
        getConfiguration() {
            if (!state.configured) return null;
            return {
                device: state.device,
                format: state.format,
                alphaMode: state.alphaMode,
                usage: state.usage,
            };
        },
    };
}

function __trueosMakeNavigatorGpu() {
    return {
        getPreferredCanvasFormat() {
            return __trueosWebGpuState.preferredCanvasFormat;
        },
        get wgslLanguageFeatures() {
            return new Set();
        },
        async requestAdapter(opts = {}) {
            return __trueosMakeGpuAdapter(opts || {});
        },
    };
}

__trueosInstallGpuConstants(G);

__trueosInstallSimpleDomCanvas(G, __trueosMakeGpuCanvasContext);

if (!G.navigator) {
    let nav = null;
    try {
        const navNative = await import('trueos:browser_navigator');
        let webgpuNative = null;
        try {
            webgpuNative = await import('trueos:browser_webgpu');
        } catch (_) {
            webgpuNative = null;
        }
        const userAgent = String(navNative.getUserAgent());
        const platform = String(navNative.getPlatform());
        const language = String(navNative.getLanguage());
        const vendor = String(navNative.getVendor());
        const hardwareConcurrency = Number(navNative.getHardwareConcurrency()) || 1;
        const onLine = !!navNative.isOnline();
        if (webgpuNative) {
            try {
                const fmt = String(webgpuNative.getPreferredCanvasFormat() || '').trim();
                if (fmt) __trueosWebGpuState.preferredCanvasFormat = fmt;
                const backend = String(webgpuNative.getBackendName() || '').trim();
                if (backend) __trueosWebGpuState.backendName = backend;
            } catch (_) {
                // Keep defaults.
            }
        }

        nav = {
            userAgent,
            platform,
            language,
            languages: [language],
            vendor,
            hardwareConcurrency,
            onLine,
            gpu: __trueosMakeNavigatorGpu(),
        };
    } catch (_) {
        nav = {
            userAgent: 'Mozilla/5.0 (TRUEOS; QuickJS)',
            platform: 'TRUEOS',
            language: 'en-US',
            languages: ['en-US'],
            vendor: 'TRUEOS',
            hardwareConcurrency: 1,
            onLine: true,
            gpu: __trueosMakeNavigatorGpu(),
        };
    }

    if (!nav.gpu) nav.gpu = __trueosMakeNavigatorGpu();

    try {
        Object.defineProperty(G, 'navigator', {
            value: nav,
            configurable: true,
            enumerable: true,
            writable: false,
        });
    } catch (_) {
        G.navigator = nav;
    }
}

if (!G.window.navigator) G.window.navigator = G.navigator;

// Some runtimes preinstall navigator without WebGPU. Ensure gpu is always present.
if (G.navigator && !G.navigator.gpu) {
    try {
        Object.defineProperty(G.navigator, 'gpu', {
            value: __trueosMakeNavigatorGpu(),
            configurable: true,
            enumerable: true,
            writable: false,
        });
    } catch (_) {
        G.navigator.gpu = __trueosMakeNavigatorGpu();
    }
}

if (G.window && G.window.navigator && !G.window.navigator.gpu) {
    try {
        Object.defineProperty(G.window.navigator, 'gpu', {
            value: __trueosMakeNavigatorGpu(),
            configurable: true,
            enumerable: true,
            writable: false,
        });
    } catch (_) {
        G.window.navigator.gpu = __trueosMakeNavigatorGpu();
    }
}

if (!G.fetch) {
  G.fetch = async () => ({ text: async () => '<html><body><h1>TRUEOS Browser</h1></body></html>' });
}

await import('/qjs/browser/main.mjs');
"#);

        if !eval_or_log(
            ctx,
            &init_script,
            init_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
            "browser-init",
        ) {
            qjs::workers::terminate_all_for_context(ctx);
            let _ = pump_runtime_once(rt, ctx);
            qjs::async_ops::drain_all_for_context(ctx);
            qjs::workers::drain_all_for_context(ctx);
            qjs::timers::drain_all_for_context(ctx);
            qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
            drop(vm);
            trueos_cabi_gfx_present_owner_set(0);
            WEBGPU_BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
            return;
        }

        let mouse_pump_filename = b"<pixi-browser-mouse-pump>\0";
        let mouse_pump_script =
            b"var G=(typeof globalThis!=='undefined')?globalThis:this; if (G.__trueos_mouse_pump) G.__trueos_mouse_pump(); if (G.__trueos_raf_pump) G.__trueos_raf_pump();";

        // Browser loop: poll host events + async jobs + mouse bridge.
        loop {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            let _ = eval_or_log(
                ctx,
                mouse_pump_script,
                mouse_pump_filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_GLOBAL,
                "mouse-pump",
            );
            Timer::after(EmbassyDuration::from_millis(16)).await;
        }

        qjs::workers::terminate_all_for_context(ctx);
        let _ = pump_runtime_once(rt, ctx);
        qjs::async_ops::drain_all_for_context(ctx);
        qjs::workers::drain_all_for_context(ctx);
        qjs::timers::drain_all_for_context(ctx);
        qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
        drop(vm);
    }

    unsafe { trueos_cabi_gfx_present_owner_set(0) };
    WEBGPU_BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
    log_str("qjs-browser: stopped\n");
}
