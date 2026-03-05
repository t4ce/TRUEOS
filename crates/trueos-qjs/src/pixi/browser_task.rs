#![cfg(feature = "trueos")]

use alloc::vec::Vec;
use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;
use crate::pixi::browser_canvas::SIMPLE_DOM_CANVAS_SHIM_JS;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_gfx_present_owner_set(owner: u32);
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
                return {
                    __id: __trueosWebGpuState.nextId++,
                    __cmds: cmds.slice(),
                };
            },
        };
    };
}

if (typeof G.__trueosMakeGpuQueue !== 'function') {
    G.__trueosMakeGpuQueue = function __trueosMakeGpuQueue(_device) {
        return {
            __id: __trueosWebGpuState.nextId++,
            submit(_cmdBuffers) {},
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
            writeTexture() {},
            async onSubmittedWorkDone() { return; },
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
