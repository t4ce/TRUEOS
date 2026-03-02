#![cfg(feature = "trueos")]

use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;

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
        let canvas_shim_filename = b"<pixi-browser-canvas-shim>\0";
        let canvas_renderer_filename = b"<pixi-browser-canvas-renderer-shim>\0";
        let init_script = br#"
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
  G.requestAnimationFrame = (cb) => {
    Promise.resolve().then(() => cb(Date.now()));
    return 1;
  };
}
if (!G.cancelAnimationFrame) G.cancelAnimationFrame = () => {};

const __trueosWebGpuState = {
    enabled: true,
    preferredCanvasFormat: 'bgra8unorm',
    backendName: 'trueos-cmd-stream',
    nextId: 1,
};
G.__trueosWebGpuState = __trueosWebGpuState;

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
            return data.buffer.slice(o, end);
        },
        unmap() {
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
            return {
                __id: __trueosWebGpuState.nextId++,
                __desc: d,
                finish() { return { __id: __trueosWebGpuState.nextId++, __bundle: true }; },
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
            if (!__trueosWebGpuState.enabled) return null;
            return __trueosMakeGpuAdapter(opts || {});
        },
    };
}

__trueosInstallGpuConstants(G);

function __trueosInstallWebGlGlobals(T) {
    if (typeof T.WebGLRenderingContext === 'undefined') {
        T.WebGLRenderingContext = function WebGLRenderingContext() {};
    }
    if (typeof T.WebGL2RenderingContext === 'undefined') {
        T.WebGL2RenderingContext = function WebGL2RenderingContext() {};
    }
}

function __trueosMakeWebGlContext(canvas, isWebGl2) {
    const gl = {
        canvas,
        // Common constants used by capability checks.
        MAX_TEXTURE_IMAGE_UNITS: 0x8872,
        MAX_COMBINED_TEXTURE_IMAGE_UNITS: 0x8B4D,
        MAX_TEXTURE_SIZE: 0x0D33,
        MAX_CUBE_MAP_TEXTURE_SIZE: 0x851C,
        MAX_RENDERBUFFER_SIZE: 0x84E8,
        MAX_VERTEX_ATTRIBS: 0x8869,
        MAX_VERTEX_UNIFORM_VECTORS: 0x8DFB,
        MAX_VARYING_VECTORS: 0x8DFC,
        MAX_FRAGMENT_UNIFORM_VECTORS: 0x8DFD,
        MAX_VIEWPORT_DIMS: 0x0D3A,
        ALIASED_LINE_WIDTH_RANGE: 0x846E,
        UNMASKED_VENDOR_WEBGL: 0x9245,
        UNMASKED_RENDERER_WEBGL: 0x9246,
        VERSION: 0x1F02,
        SHADING_LANGUAGE_VERSION: 0x8B8C,
        VENDOR: 0x1F00,
        RENDERER: 0x1F01,
        FRAGMENT_SHADER: 0x8B30,
        HIGH_FLOAT: 0x8DF2,
        COMPILE_STATUS: 0x8B81,
        // Misc constants some code may touch.
        BLEND: 0x0BE2,
        DEPTH_TEST: 0x0B71,
        CULL_FACE: 0x0B44,
        POLYGON_OFFSET_FILL: 0x8037,
        CW: 0x0900,
        CCW: 0x0901,
        FUNC_ADD: 0x8006,
        TEXTURE0: 0x84C0,
        TEXTURE_2D: 0x0DE1,
        RGBA: 0x1908,
        UNSIGNED_BYTE: 0x1401,
        FRAMEBUFFER: 0x8D40,
        COLOR_BUFFER_BIT: 0x00004000,
        DEPTH_BUFFER_BIT: 0x00000100,
        TRIANGLES: 0x0004,
        UNSIGNED_SHORT: 0x1403,
        UNSIGNED_INT: 0x1405,
        FLOAT: 0x1406,
        BYTE: 0x1400,
        SHORT: 0x1402,
        HALF_FLOAT: 0x140B,
        RGBA8: 0x8058,
        STENCIL_INDEX8: 0x8D48,
        DEPTH_COMPONENT: 0x1902,
        DEPTH_STENCIL: 0x84F9,
        DEPTH_COMPONENT16: 0x81A5,
        DEPTH_COMPONENT24: 0x81A6,
        DEPTH24_STENCIL8: 0x88F0,
        DEPTH_COMPONENT32F: 0x8CAC,
        DEPTH32F_STENCIL8: 0x8CAD,
        R8: 0x8229,
        R8_SNORM: 0x8F94,
        R8UI: 0x8232,
        R8I: 0x8231,
        R16UI: 0x8234,
        R16I: 0x8233,
        R16F: 0x822D,
        RG8: 0x822B,
        RG8_SNORM: 0x8F95,
        RG8UI: 0x8238,
        RG8I: 0x8237,
        R32UI: 0x8236,
        R32I: 0x8235,
        R32F: 0x822E,
        RG16UI: 0x823A,
        RG16I: 0x8239,
        RG16F: 0x822F,
        RGBA8_SNORM: 0x8F97,
        RGBA8UI: 0x8D7C,
        RGBA8I: 0x8D8E,
        RGB9_E5: 0x8C3D,
        RGB10_A2: 0x8059,
        R11F_G11F_B10F: 0x8C3A,
        RG32UI: 0x823C,
        RG32I: 0x823B,
        RG32F: 0x8230,
        RGBA16UI: 0x8D76,
        RGBA16I: 0x8D88,
        RGBA16F: 0x881A,
        RGBA32UI: 0x8D70,
        RGBA32I: 0x8D82,
        RGBA32F: 0x8814,
        UNPACK_FLIP_Y_WEBGL: 0x9240,
        UNPACK_PREMULTIPLY_ALPHA_WEBGL: 0x9241,
        UNPACK_ALIGNMENT: 0x0CF5,
        // Capability probes.
        getContextAttributes() {
            return {
                alpha: true,
                antialias: false,
                depth: true,
                stencil: true,
                premultipliedAlpha: true,
                preserveDrawingBuffer: false,
            };
        },
        isContextLost() { return false; },
        getParameter(p) {
            switch (p) {
                case this.MAX_TEXTURE_IMAGE_UNITS:
                case this.MAX_COMBINED_TEXTURE_IMAGE_UNITS:
                    return 16;
                case this.MAX_TEXTURE_SIZE:
                case this.MAX_CUBE_MAP_TEXTURE_SIZE:
                case this.MAX_RENDERBUFFER_SIZE:
                    return 8192;
                case this.MAX_VERTEX_ATTRIBS:
                    return 16;
                case this.MAX_VERTEX_UNIFORM_VECTORS:
                case this.MAX_VARYING_VECTORS:
                case this.MAX_FRAGMENT_UNIFORM_VECTORS:
                    return 1024;
                case this.MAX_VIEWPORT_DIMS:
                    return [8192, 8192];
                case this.ALIASED_LINE_WIDTH_RANGE:
                    return [1, 1];
                case this.VERSION:
                    return isWebGl2 ? 'WebGL 2.0 (TRUEOS stub)' : 'WebGL 1.0 (TRUEOS stub)';
                case this.SHADING_LANGUAGE_VERSION:
                    return isWebGl2 ? 'WebGL GLSL ES 3.00 (TRUEOS stub)' : 'WebGL GLSL ES 1.00 (TRUEOS stub)';
                case this.VENDOR:
                case this.UNMASKED_VENDOR_WEBGL:
                    return 'TRUEOS';
                case this.RENDERER:
                case this.UNMASKED_RENDERER_WEBGL:
                    return 'TRUEOS WebGL probe stub';
                default:
                    return 0;
            }
        },
        getSupportedExtensions() { return []; },
        getShaderPrecisionFormat() {
            return { rangeMin: 127, rangeMax: 127, precision: 23 };
        },
        getExtension(name) {
            const n = String(name || '').toUpperCase();
            if (n === 'WEBGL_LOSE_CONTEXT') {
                return {
                    loseContext() {},
                    restoreContext() {},
                };
            }
            if (n === 'EXT_TEXTURE_FILTER_ANISOTROPIC' || n === 'WEBKIT_EXT_TEXTURE_FILTER_ANISOTROPIC' || n === 'MOZ_EXT_TEXTURE_FILTER_ANISOTROPIC') {
                return {
                    MAX_TEXTURE_MAX_ANISOTROPY_EXT: 0x84FF,
                    TEXTURE_MAX_ANISOTROPY_EXT: 0x84FE,
                };
            }
            return null;
        },
        // No-op API surface to satisfy probes and defensive calls.
        createTexture() { return {}; },
        deleteTexture() {},
        bindTexture() {},
        texImage2D() {},
        texSubImage2D() {},
        compressedTexImage2D() {},
        texParameteri() {},
        pixelStorei() {},
        activeTexture() {},
        generateMipmap() {},
        createSampler() { return {}; },
        samplerParameteri() {},
        bindSampler() {},
        createFramebuffer() { return {}; },
        bindFramebuffer() {},
        createBuffer() { return {}; },
        bindBuffer() {},
        bufferData() {},
        bufferSubData() {},
        createProgram() { return {}; },
        createShader() { return {}; },
        deleteShader() {},
        shaderSource() {},
        compileShader() {},
        getShaderParameter() { return true; },
        getShaderInfoLog() { return ''; },
        attachShader() {},
        linkProgram() {},
        getProgramParameter() { return true; },
        getProgramInfoLog() { return ''; },
        useProgram() {},
        getAttribLocation() { return 0; },
        getUniformLocation() { return {}; },
        uniform1f() {},
        uniform1i() {},
        uniform2f() {},
        uniform3f() {},
        uniform4f() {},
        uniformMatrix3fv() {},
        uniformMatrix4fv() {},
        enable() {},
        disable() {},
        blendFunc() {},
        blendFuncSeparate() {},
        blendEquationSeparate() {},
        depthMask() {},
        frontFace() {},
        polygonOffset() {},
        viewport() {},
        clearColor() {},
        clear() {},
        drawArrays() {},
        drawElements() {},
        createVertexArray() { return {}; },
        bindVertexArray() {},
        vertexAttribPointer() {},
        enableVertexAttribArray() {},
        disableVertexAttribArray() {},
        readPixels() {},
    };
    return gl;
}

__trueosInstallWebGlGlobals(G);

const mkNode = () => ({
  style: {},
  children: [],
  parentNode: null,
  ownerDocument: null,
  eventMode: 'none',
  appendChild(ch) { this.children.push(ch); ch.parentNode = this; return ch; },
  removeChild(ch) { this.children = this.children.filter((x) => x !== ch); ch.parentNode = null; return ch; },
  addEventListener() {},
  removeEventListener() {},
  dispatchEvent() { return true; },
  setAttribute() {},
  getAttribute() { return null; },
  contains(node) { if (node === this) return true; for (const c of this.children) { if (c && typeof c.contains === 'function' && c.contains(node)) return true; } return false; },
  getBoundingClientRect() { return { x: 0, y: 0, left: 0, top: 0, width: this.width || 0, height: this.height || 0 }; },
});

if (!G.document) {
  const doc = mkNode();
  doc.ownerDocument = doc;
  doc.documentElement = mkNode();
  doc.head = mkNode();
  doc.body = mkNode();
  doc.documentElement.ownerDocument = doc;
  doc.head.ownerDocument = doc;
  doc.body.ownerDocument = doc;
  doc.documentElement.appendChild(doc.head);
  doc.documentElement.appendChild(doc.body);

  const mkCanvas = () => {
    const c = mkNode();
    c.tagName = 'CANVAS';
    c.width = G.window.innerWidth | 0;
    c.height = G.window.innerHeight | 0;
        let gpuCtx = null;
        let webglCtx = null;
        let webgl2Ctx = null;
    c.getContext = (kind) => {
            const k = String(kind || '').toLowerCase();
            if (k === 'webgpu') {
                if (!gpuCtx) gpuCtx = __trueosMakeGpuCanvasContext(c);
                return gpuCtx;
            }
            if (k === 'webgl2') {
                if (!webgl2Ctx) webgl2Ctx = __trueosMakeWebGlContext(c, true);
                return webgl2Ctx;
            }
            if (k === 'webgl' || k === 'experimental-webgl') {
                if (!webglCtx) webglCtx = __trueosMakeWebGlContext(c, false);
                return webglCtx;
            }
                        if (k !== '2d') return null;
                        if (typeof G.__trueosMakeCanvas2dContext === 'function') {
                                return G.__trueosMakeCanvas2dContext();
                        }
                        return {
                                font: '16px sans-serif',
                                measureText: (s) => ({ width: (String(s).length || 0) * 8 }),
                                clearRect() {},
                                fillRect() {},
                                beginPath() {},
                                moveTo() {},
                                lineTo() {},
                                stroke() {},
                                fill() {},
                        };
    };
    return c;
  };

  doc.createElement = (tag) => {
    const t = String(tag || '').toLowerCase();
    const n = t === 'canvas' ? mkCanvas() : mkNode();
    n.tagName = String(tag || '').toUpperCase();
    n.ownerDocument = doc;
    return n;
  };
  doc.getElementById = () => null;
  doc.addEventListener = () => {};
  doc.removeEventListener = () => {};
  doc.dispatchEvent = () => true;
  G.document = doc;
}

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
                __trueosWebGpuState.enabled = !!webgpuNative.isAvailable();
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
"#;

        if !eval_or_log(
            ctx,
            qjs::browser_canvas::CANVAS_2D_SHIM_JS,
            canvas_shim_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
            "browser-canvas-shim",
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

        if !eval_or_log(
            ctx,
            qjs::browser_canvas_renderer::CANVAS_RENDERER_SHIM_JS,
            canvas_renderer_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
            "browser-canvas-renderer-shim",
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

        if !eval_or_log(
            ctx,
            init_script,
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
            b"var G=(typeof globalThis!=='undefined')?globalThis:this; if (G.__trueos_mouse_pump) G.__trueos_mouse_pump();";

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
