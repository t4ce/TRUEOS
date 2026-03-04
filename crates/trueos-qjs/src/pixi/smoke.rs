#![cfg(feature = "trueos")]

use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_gfx_present_owner_set(owner: u32);
}

static PIXI_SMOKE_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static PIXI_CDN_PRELOAD_DONE: AtomicBool = AtomicBool::new(false);

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
                qjs::qjs_diag::dump_last_exception(ctx, "pixi-smoke pending-job");
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
        log_str("qjs-pixi-smoke: ");
        log_str(label);
        log_str(" JS_Eval exception\n");
        qjs::qjs_diag::dump_last_exception(ctx, "pixi-smoke eval");
        return false;
    }
    qjs::js_free_value(ctx, val);
    true
}

pub async fn preload_pixi_cdn_once() -> bool {
    if PIXI_CDN_PRELOAD_DONE.load(Ordering::Acquire) {
        return true;
    }
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-pixi-preload: JS_NewRuntime failed\n");
                return false;
            }
        };
        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        qjs::node::install_globals(ctx);

        let preload_filename = b"<pixi-cdn-preload>\0";
        let preload_script = br#"
    await import('/qjs/cdn/8d2f5f0bba6a6702.mjs');
    await import('/qjs/vendor/parse5.mjs');
    await import('/qjs/vendor/yoga.mjs');
    "#;
        if !eval_or_log(
            ctx,
            preload_script,
            preload_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
            "cdn-preload",
        ) {
            drop(vm);
            return false;
        }
        for _ in 0..120 {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
        qjs::workers::terminate_all_for_context(ctx);
        let _ = pump_runtime_once(rt, ctx);
        qjs::async_ops::drain_all_for_context(ctx);
        qjs::workers::drain_all_for_context(ctx);
        qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
        drop(vm);
    }
    PIXI_CDN_PRELOAD_DONE.store(true, Ordering::Release);
    log_str("qjs-pixi-preload: cached /qjs/cdn/8d2f5f0bba6a6702.mjs\n");
    log_str("qjs-pixi-preload: cached /qjs/vendor/parse5.mjs\n");
    log_str("qjs-pixi-preload: cached /qjs/vendor/yoga.mjs\n");
    true
}

#[embassy_executor::task]
pub async fn boot_pixi_scene_smoke_task() {
    if PIXI_SMOKE_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-pixi-smoke: already running\n");
        return;
    }

    log_str("qjs-pixi-smoke: starting (render bridge on)\n");
    unsafe { trueos_cabi_gfx_present_owner_set(1) };
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-pixi-smoke: JS_NewRuntime failed\n");
                unsafe { trueos_cabi_gfx_present_owner_set(0) };
                PIXI_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };
        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        qjs::node::install_globals(ctx);

        let init_filename = b"<pixi-smoke-init>\0";
        let init_script = br#"
const G = (typeof globalThis !== 'undefined') ? globalThis : this;

import * as cmd from 'cmd_stream';
let browserContext = null;
try {
    browserContext = await import('trueos:browser_context');
} catch {
    browserContext = null;
}
let YogaCompat = null;
try {
    YogaCompat = await import('trueos:yoga');
} catch {
    // Keep smoke startup alive if Yoga import fails.
}
try {
    await import('trueos:threejs');
} catch {
    // Keep smoke startup alive if Three.js import fails.
}
const PIXI = await import('/qjs/vendor/pixi.mjs');
const parse5 = await import('/qjs/vendor/parse5.mjs');
const parse5ButtonWidget = await import('/qjs/browser/widgets/button.mjs');
const parse5CheckboxWidget = await import('/qjs/browser/widgets/checkbox.mjs');
const W = Number((G.window && G.window.innerWidth) || 1280);
const H = Number((G.window && G.window.innerHeight) || 800);
const CX = W * 0.5;
const CY = H * 0.5;
const RING_COUNT = 8;

const uiRoot = new PIXI.Container();

const panel = new PIXI.Sprite(PIXI.Texture.WHITE);
panel.anchor.set(0, 0);
panel.tint = 0xF4F7FB;
panel.alpha = 1.0;
uiRoot.addChild(panel);

const header = new PIXI.Sprite(PIXI.Texture.WHITE);
header.anchor.set(0, 0);
header.tint = 0x2A84FF;
header.alpha = 0.18;
uiRoot.addChild(header);

const footer = new PIXI.Sprite(PIXI.Texture.WHITE);
footer.anchor.set(0, 0);
footer.tint = 0x00FF00;
footer.alpha = 1.0;
uiRoot.addChild(footer);

const cards = [];
for (let i = 0; i < 3; i++) {
    const c = new PIXI.Sprite(PIXI.Texture.WHITE);
    c.anchor.set(0, 0);
    c.tint = i === 1 ? 0xFF8B2E : 0x2A84FF;
    c.alpha = i === 1 ? 0.30 : 0.22;
    cards.push(c);
    uiRoot.addChild(c);
}

function layoutUiBox(w, h) {
    const pad = 24;
    const gap = 14;
    const headerH = 70;
    const footerH = 52;
    const rowY = pad + headerH + gap;
    const rowH = Math.max(120, h - rowY - footerH - pad - gap);
    const cardGap = 12;
    const cardW = Math.max(80, ((w - pad * 2) - cardGap * 2) / 3);

    panel.position.set(0, 0);
    panel.width = w;
    panel.height = h;

    header.position.set(pad, pad);
    header.width = Math.max(1, w - pad * 2);
    header.height = headerH;

    for (let i = 0; i < cards.length; i++) {
        const x = pad + i * (cardW + cardGap);
        cards[i].position.set(x, rowY);
        cards[i].width = cardW;
        cards[i].height = rowH;
    }

    footer.position.set(pad, h - pad - footerH);
    footer.width = Math.max(1, w - pad * 2);
    footer.height = footerH;

    return {
        pad,
        headerH,
        rowY,
        rowH,
        cardW,
        footerY: h - pad - footerH,
    };
}

const root = new PIXI.Container();
const tex = PIXI.Texture.WHITE;
const bg = new PIXI.Sprite(tex);
bg.anchor.set(0.5, 0.5);
bg.width = 420;
bg.height = 260;
bg.tint = 0x2A84FF;
bg.alpha = 0.58;

const fg = new PIXI.Sprite(tex);
fg.anchor.set(0.5, 0.5);
fg.width = 360;
fg.height = 220;
fg.tint = 0xFF8B2E;
fg.alpha = 0.56;

root.addChild(bg);
root.addChild(fg);

const labels = [
  { text: 'true',  color: 0x101010, size: 12, phase: 0.00, r: 220 },
  { text: 'os',    color: 0x101010, size: 12, phase: 0.78, r: 220 },
  { text: 'wgpu',  color: 0x003366, size: 12, phase: 1.57, r: 220 },
  { text: 'pixi',  color: 0x660000, size: 12, phase: 2.35, r: 220 },
  { text: 'virgl', color: 0x203000, size: 12, phase: 3.14, r: 220 },
  { text: 'demo',  color: 0x303030, size: 12, phase: 3.92, r: 220 },
];

const globalCursors = [
    { id: 1, color: 0x111111, posX: 0.31, posY: 0.58 },
    { id: 2, color: 0x2563EB, posX: 0.36, posY: 0.54 },
    { id: 3, color: 0x16A34A, posX: 0.42, posY: 0.62 },
    { id: 4, color: 0xDC2626, posX: 0.47, posY: 0.57 },
];

// Ported AI cursor motion profile: single autonomous cursor patrol.
const aiCursor = {
    color: 0x7C3AED,
    centerX: 0.75,
    centerY: 0.25,
    radius: 120,
    speed: 0.9,
    phase: 0.0,
};

const menuLabels = ['Copy', 'Paste', 'Close'];
const menuItemW = 140;
const menuItemH = 28;
const menuPad = 6;
const menuBorderW = 2;

const SIMPLE_PARSE5_HTML = `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>HTML Only Visual Elements</title>
</head>
<body>

        <button type="submit">Submit</button>

</body>
</html>`;

function normalizeWhitespace(s) {
    return String(s ?? '').replace(/\s+/g, ' ').trim();
}

function attrsToMap(attrs) {
    const out = {};
    if (!Array.isArray(attrs)) return out;
    for (let i = 0; i < attrs.length; i++) {
        const a = attrs[i];
        if (!a || typeof a.name !== 'string') continue;
        out[a.name] = String(a.value ?? '');
    }
    return out;
}

function extractText(node) {
    if (!node || typeof node !== 'object') return '';
    const name = String(node.nodeName || '').toLowerCase();
    if (name === '#text') return String(node.value ?? '');
    const children = Array.isArray(node.childNodes) ? node.childNodes : [];
    let out = '';
    for (let i = 0; i < children.length; i++) out += extractText(children[i]);
    return out;
}

function getBodyNode(doc) {
    const queue = [doc];
    while (queue.length > 0) {
        const cur = queue.shift();
        if (!cur || typeof cur !== 'object') continue;
        const tag = String(cur.tagName || cur.nodeName || '').toLowerCase();
        if (tag === 'body') return cur;
        const children = Array.isArray(cur.childNodes) ? cur.childNodes : [];
        for (let i = 0; i < children.length; i++) queue.push(children[i]);
    }
    return doc;
}

function toSimpleRenderTree(node, path = '0') {
    const out = [];
    const children = Array.isArray(node?.childNodes) ? node.childNodes : [];
    for (let i = 0; i < children.length; i++) {
        const ch = children[i];
        const childPath = `${path}.${i}`;
        const nodeName = String(ch?.nodeName || '').toLowerCase();
        const tagName = String(ch?.tagName || '').toLowerCase();

        if (nodeName === '#text') {
            const text = normalizeWhitespace(String(ch?.value ?? ''));
            if (text.length > 0) out.push({ kind: 'text', text });
            continue;
        }

        if (tagName === 'button') {
            out.push({
                kind: 'block',
                key: `${childPath}:button`,
                tagName: 'button',
                attrs: attrsToMap(ch?.attrs),
                children: [{ kind: 'text', text: normalizeWhitespace(extractText(ch) || 'Submit') }],
            });
            continue;
        }

        if (Array.isArray(ch?.childNodes) && ch.childNodes.length > 0) {
            out.push(...toSimpleRenderTree(ch, childPath));
        }
    }
    return out;
}

function makeYogaNodeAdapter(Y, id) {
    return {
        setFlexDirection(v) { Y.nodeSetFlexDirection?.(id, v); },
        setPadding(edge, value) { Y.nodeSetPadding?.(id, edge, value); },
        setMinHeight(value) { Y.nodeSetMinHeight?.(id, value); },
        setMinWidth(value) { Y.nodeSetMinWidth?.(id, value); },
        setAlignItems(v) { Y.nodeSetAlignItems?.(id, v); },
        setJustifyContent(v) { Y.nodeSetJustifyContent?.(id, v); },
        setWidth(value) { Y.nodeSetWidth?.(id, value); },
        setHeight(value) { Y.nodeSetHeight?.(id, value); },
        setMargin(edge, value) { Y.nodeSetMargin?.(id, edge, value); },
    };
}

const SIMPLE_PARSE5_DOC = (parse5.parse?.(SIMPLE_PARSE5_HTML))
    || (parse5.default && parse5.default.parse && parse5.default.parse(SIMPLE_PARSE5_HTML))
    || null;
const SIMPLE_PARSE5_BODY = SIMPLE_PARSE5_DOC ? (getBodyNode(SIMPLE_PARSE5_DOC) || SIMPLE_PARSE5_DOC) : null;
const SIMPLE_PARSE5_RENDER_NODES = SIMPLE_PARSE5_BODY ? toSimpleRenderTree(SIMPLE_PARSE5_BODY, '0') : [];
const SIMPLE_PARSE5_BUTTON_NODE = SIMPLE_PARSE5_RENDER_NODES.find((n) => n && n.kind === 'block' && n.tagName === 'button') || null;
const SIMPLE_PARSE5_BUTTON_TEXT = SIMPLE_PARSE5_BUTTON_NODE
    ? normalizeWhitespace(((SIMPLE_PARSE5_BUTTON_NODE.children || []).map((c) => c?.text || '').join(' ')) || 'Submit')
    : 'Submit';

function buildSimpleParse5YogaButtonLayout(viewW, viewH) {
    const fallback = {
        x: 0,
        y: Math.max(0, (viewH - 34) * 0.5),
        w: Math.max(120, Math.min(220, viewW)),
        h: 34,
    };

    if (!YogaCompat || !SIMPLE_PARSE5_BUTTON_NODE) {
        return { button: fallback, usedYoga: false, nodeCount: SIMPLE_PARSE5_RENDER_NODES.length };
    }

    const Y = YogaCompat;
    const cfg = Y.configCreate ? (Y.configCreate() >>> 0) : 0;
    const root = Y.nodeCreate ? (Y.nodeCreate(cfg) >>> 0) : 0;
    if (!root) {
        return { button: fallback, usedYoga: false, nodeCount: SIMPLE_PARSE5_RENDER_NODES.length };
    }

    const btn = Y.nodeCreate ? (Y.nodeCreate(cfg) >>> 0) : 0;
    if (!btn) {
        Y.nodeFreeRecursive?.(root);
        return { button: fallback, usedYoga: false, nodeCount: SIMPLE_PARSE5_RENDER_NODES.length };
    }

    const rootA = makeYogaNodeAdapter(Y, root);
    rootA.setFlexDirection(Y.FLEX_DIRECTION_COLUMN ?? 0);
    rootA.setAlignItems(Y.ALIGN_STRETCH ?? 4);
    rootA.setWidth(Math.max(1, viewW));
    rootA.setHeight(Math.max(1, viewH));
    rootA.setPadding(Y.EDGE_LEFT ?? 0, 8);
    rootA.setPadding(Y.EDGE_RIGHT ?? 2, 8);
    rootA.setPadding(Y.EDGE_TOP ?? 1, 8);
    rootA.setPadding(Y.EDGE_BOTTOM ?? 3, 8);

    const btnA = makeYogaNodeAdapter(Y, btn);
    parse5ButtonWidget.applyYogaDefaultsButton(btnA, Y);
    Y.nodeInsertChild?.(root, btn, Y.nodeGetChildCount ? Y.nodeGetChildCount(root) : 0);

    Y.nodeCalculateLayout?.(root, Math.max(1, viewW), Math.max(1, viewH), Y.DIRECTION_LTR ?? 1);

    const out = {
        x: Number(Y.nodeGetComputedLeft?.(btn) ?? fallback.x),
        y: Number(Y.nodeGetComputedTop?.(btn) ?? fallback.y),
        w: Number(Y.nodeGetComputedWidth?.(btn) ?? fallback.w),
        h: Number(Y.nodeGetComputedHeight?.(btn) ?? fallback.h),
    };

    Y.nodeFreeRecursive?.(root);

    if (!Number.isFinite(out.w) || out.w < 1) out.w = fallback.w;
    if (!Number.isFinite(out.h) || out.h < 1) out.h = fallback.h;
    if (!Number.isFinite(out.x)) out.x = fallback.x;
    if (!Number.isFinite(out.y)) out.y = fallback.y;

    return { button: out, usedYoga: true, nodeCount: SIMPLE_PARSE5_RENDER_NODES.length };
}

const parse5ButtonTheme = {
    control: {
        button: {
            fill: 0x1E3A5F,
            hoverFill: 0x2A84FF,
            activeFill: 0x8B2E2E,
            border: 0x0F2742,
            radius: 8,
        },
    },
};

const parse5CheckboxTheme = {
    control: {
        background: 0xFFFFFF,
        border: 0x1A3247,
        accent: 0x2A84FF,
    },
};

const parse5ButtonModel = {
    x: 0,
    y: 0,
    w: 196,
    h: 34,
    fill: parse5ButtonTheme.control.button.fill,
    border: parse5ButtonTheme.control.button.border,
    clickCount: 0,
    pressSeq: new Map(),
    activeUntil: 0.0,
    mode: 'out',
};

const parse5ButtonGraphics = {
    clear() {
        return this;
    },
    roundRect(_x, _y, w, h, _r) {
        parse5ButtonModel.w = Math.max(1, Number(w));
        parse5ButtonModel.h = Math.max(1, Number(h));
        return this;
    },
    rect(_x, _y, w, h) {
        parse5ButtonModel.w = Math.max(1, Number(w));
        parse5ButtonModel.h = Math.max(1, Number(h));
        return this;
    },
    fill(v) {
        if (typeof v === 'number') {
            parse5ButtonModel.fill = v >>> 0;
        } else if (v && typeof v.color === 'number') {
            parse5ButtonModel.fill = v.color >>> 0;
        }
        return this;
    },
    stroke(v) {
        if (typeof v === 'number') {
            parse5ButtonModel.border = v >>> 0;
        } else if (v && typeof v.color === 'number') {
            parse5ButtonModel.border = v.color >>> 0;
        }
        return this;
    },
};

const parse5ButtonContainer = {
    eventMode: 'none',
    cursor: 'default',
    __handlers: {},
    removeAllListeners() {
        this.__handlers = {};
    },
    on(name, fn) {
        this.__handlers[name] = fn;
        return this;
    },
};

parse5ButtonWidget.renderButton({
    container: parse5ButtonContainer,
    graphics: parse5ButtonGraphics,
    w: parse5ButtonModel.w,
    h: parse5ButtonModel.h,
    theme: parse5ButtonTheme,
});

const parse5CheckboxModel = {
    x: 0,
    y: 0,
    w: 16,
    h: 16,
    checked: false,
    indeterminate: false,
    pressSeq: new Map(),
};

const parse5CheckboxGraphics = {
    clear() { return this; },
    rect(_x, _y, _w, _h) { return this; },
    fill(_v) { return this; },
    stroke(_v) { return this; },
    moveTo(_x, _y) { return this; },
    lineTo(_x, _y) { return this; },
};

const parse5CheckboxContainer = {
    eventMode: 'none',
    cursor: 'default',
    __handlers: {},
    removeAllListeners() {
        this.__handlers = {};
    },
    on(name, fn) {
        this.__handlers[name] = fn;
        return this;
    },
};

parse5CheckboxWidget.renderCheckbox({
    container: parse5CheckboxContainer,
    graphics: parse5CheckboxGraphics,
    w: parse5CheckboxModel.w,
    h: parse5CheckboxModel.h,
    theme: parse5CheckboxTheme,
    state: parse5CheckboxModel,
});

// Future hook: kernel-side cursor tilt targets from hover/active UI state.
// Nothing calls this yet; defaults keep cursors upright.
const cursorTilt = {
    byId: new Map(),
    setState(id, hovered, active) {
        const target = (hovered || active) ? (Math.PI * 0.25) : 0.0;
        this.byId.set(id | 0, {
            target,
            active: !!active,
            hovered: !!hovered,
        });
    },
    step(id, dt) {
        const key = id | 0;
        const rec = this.byId.get(key) || { target: 0.0, rot: 0.0 };
        const rot0 = Number(rec.rot || 0.0);
        const target = Number(rec.target || 0.0);
        const speed = 14.0;
        const k = Math.max(0.0, Math.min(1.0, dt * speed));
        const rot = rot0 + (target - rot0) * k;
        rec.rot = rot;
        rec.target = target;
        this.byId.set(key, rec);
        return rot;
    },
};

const MAX_QUADS = 128;
const out = new Uint8Array(12 * 6 * MAX_QUADS);
const dv = new DataView(out.buffer);
const texOut = new Uint8Array(20 * 6 * 24);
const texDv = new DataView(texOut.buffer);
const atlasTex = cmd.createAtlasTexture(1);

const proofTexW = 8;
const proofTexH = 8;
const proofTexPixels = new Uint8Array(proofTexW * proofTexH * 4);
for (let y = 0; y < proofTexH; y++) {
    for (let x = 0; x < proofTexW; x++) {
        const i = (y * proofTexW + x) * 4;
        const c = (((x >> 1) + (y >> 1)) & 1) ? 0x2A84FF : 0xFF8B2E;
        proofTexPixels[i + 0] = (c >>> 16) & 0xff;
        proofTexPixels[i + 1] = (c >>> 8) & 0xff;
        proofTexPixels[i + 2] = c & 0xff;
        proofTexPixels[i + 3] = 255;
    }
}
const proofTex = cmd.createTextureRgba(proofTexW, proofTexH, proofTexPixels);

G.__pixi_smoke = {
  root,
  bg,
  fg,
    uiRoot,
    panel,
    header,
    footer,
    cards,
  labels,
    globalCursors,
        aiCursor,
        cursorRuntime: new Map(),
        cursorTilt,
          menuClickSeq: new Map(),
  atlasTex,
    proofTex,
    parse5ButtonModel,
        parse5YogaDemo: {
                html: SIMPLE_PARSE5_HTML,
                buttonText: SIMPLE_PARSE5_BUTTON_TEXT,
                renderNodeCount: SIMPLE_PARSE5_RENDER_NODES.length,
                usedYoga: false,
        },
        parse5CheckboxModel,
  out,
  dv,
    texOut,
    texDv,
  t: 0.0,
  frame: 0,
};

// Public prep API for future kernel/browser input wiring.
G.__pixi_smoke_set_cursor_hover = function(id, hovered) {
    const s = G.__pixi_smoke;
    if (!s) return;
    const key = (id | 0);
    const prev = s.cursorTilt.byId.get(key);
    s.cursorTilt.setState(key, !!hovered, !!(prev && prev.active));
};

G.__pixi_smoke_set_cursor_active = function(id, active) {
    const s = G.__pixi_smoke;
    if (!s) return;
    const key = (id | 0);
    const prev = s.cursorTilt.byId.get(key);
    s.cursorTilt.setState(key, !!(prev && prev.hovered), !!active);
};

function writeVertex(dv, out, off, x, y, rgb, alpha) {
  const nx = (2.0 * (x / W)) - 1.0;
  const ny = 1.0 - (2.0 * (y / H));
  dv.setFloat32(off + 0, nx, true);
  dv.setFloat32(off + 4, ny, true);
  out[off + 8] = (rgb >>> 16) & 0xff;
  out[off + 9] = (rgb >>> 8) & 0xff;
  out[off + 10] = rgb & 0xff;
  out[off + 11] = alpha & 0xff;
  return off + 12;
}

function emitQuad(dv, out, off, cx, cy, w, h, rot, rgb, alpha) {
  const hw = w * 0.5;
  const hh = h * 0.5;
  const c = Math.cos(rot);
  const s = Math.sin(rot);
  const p0x = cx + (-hw * c - -hh * s);
  const p0y = cy + (-hw * s + -hh * c);
  const p1x = cx + ( hw * c - -hh * s);
  const p1y = cy + ( hw * s + -hh * c);
  const p2x = cx + ( hw * c -  hh * s);
  const p2y = cy + ( hw * s +  hh * c);
  const p3x = cx + (-hw * c -  hh * s);
  const p3y = cy + (-hw * s +  hh * c);
  off = writeVertex(dv, out, off, p0x, p0y, rgb, alpha);
  off = writeVertex(dv, out, off, p1x, p1y, rgb, alpha);
  off = writeVertex(dv, out, off, p2x, p2y, rgb, alpha);
  off = writeVertex(dv, out, off, p0x, p0y, rgb, alpha);
  off = writeVertex(dv, out, off, p2x, p2y, rgb, alpha);
  off = writeVertex(dv, out, off, p3x, p3y, rgb, alpha);
  return off;
}

function emitCursorCross(dv, out, off, cx, cy, arm, stroke, rot, rgb, alpha) {
    off = emitQuad(dv, out, off, cx, cy, arm * 2.0, stroke, rot, rgb, alpha);
    off = emitQuad(dv, out, off, cx, cy, stroke, arm * 2.0, rot, rgb, alpha);
    return off;
}

function writeTexVertex(dv, out, off, x, y, u, v, rgb, alpha) {
        const nx = (2.0 * (x / W)) - 1.0;
        const ny = 1.0 - (2.0 * (y / H));
        dv.setFloat32(off + 0, nx, true);
        dv.setFloat32(off + 4, ny, true);
        dv.setFloat32(off + 8, u, true);
        dv.setFloat32(off + 12, v, true);
        out[off + 16] = (rgb >>> 16) & 0xff;
        out[off + 17] = (rgb >>> 8) & 0xff;
        out[off + 18] = rgb & 0xff;
        out[off + 19] = alpha & 0xff;
        return off + 20;
}

function emitTexturedQuad(dv, out, off, cx, cy, w, h, rot, u0, v0, u1, v1, rgb, alpha) {
        const hw = w * 0.5;
        const hh = h * 0.5;
        const c = Math.cos(rot);
        const s = Math.sin(rot);
        const p0x = cx + (-hw * c - -hh * s);
        const p0y = cy + (-hw * s + -hh * c);
        const p1x = cx + ( hw * c - -hh * s);
        const p1y = cy + ( hw * s + -hh * c);
        const p2x = cx + ( hw * c -  hh * s);
        const p2y = cy + ( hw * s +  hh * c);
        const p3x = cx + (-hw * c -  hh * s);
        const p3y = cy + (-hw * s +  hh * c);

        off = writeTexVertex(dv, out, off, p0x, p0y, u0, v0, rgb, alpha);
        off = writeTexVertex(dv, out, off, p1x, p1y, u1, v0, rgb, alpha);
        off = writeTexVertex(dv, out, off, p2x, p2y, u1, v1, rgb, alpha);
        off = writeTexVertex(dv, out, off, p0x, p0y, u0, v0, rgb, alpha);
        off = writeTexVertex(dv, out, off, p2x, p2y, u1, v1, rgb, alpha);
        off = writeTexVertex(dv, out, off, p3x, p3y, u0, v1, rgb, alpha);
        return off;
}

function emitSpriteQuadFromPixiNode(dv, out, off, root, spr) {
    const rw = Number(root.scale?.x ?? 1.0);
    const rh = Number(root.scale?.y ?? 1.0);
    const rr = Number(root.rotation ?? 0.0);
    const rc = Math.cos(rr);
    const rs = Math.sin(rr);
    const rtx = Number(root.position?.x ?? 0.0);
    const rty = Number(root.position?.y ?? 0.0);

    const sw = Number(spr.width ?? 0.0);
    const sh = Number(spr.height ?? 0.0);
    const sx = Number(spr.position?.x ?? 0.0);
    const sy = Number(spr.position?.y ?? 0.0);
    const sr = Number(spr.rotation ?? 0.0);
    const sc = Math.cos(sr);
    const ss = Math.sin(sr);
    const sax = Number(spr.anchor?.x ?? 0.0);
    const say = Number(spr.anchor?.y ?? 0.0);

    const tint = Number(spr.tint ?? 0xFFFFFF) >>> 0;
    const alpha = Math.max(0, Math.min(255, Math.round(Number(spr.alpha ?? 1.0) * 255.0)));

    const p = [
        [-sax * sw, -say * sh],
        [(1.0 - sax) * sw, -say * sh],
        [(1.0 - sax) * sw, (1.0 - say) * sh],
        [-sax * sw, (1.0 - say) * sh],
    ];

    const outPts = [];
    for (let i = 0; i < 4; i++) {
        const lx = p[i][0];
        const ly = p[i][1];
        const x1 = lx * sc - ly * ss + sx;
        const y1 = lx * ss + ly * sc + sy;
        const x2 = x1 * rw;
        const y2 = y1 * rh;
        const x3 = x2 * rc - y2 * rs + rtx;
        const y3 = x2 * rs + y2 * rc + rty;
        outPts.push([x3, y3]);
    }

    off = writeVertex(dv, out, off, outPts[0][0], outPts[0][1], tint, alpha);
    off = writeVertex(dv, out, off, outPts[1][0], outPts[1][1], tint, alpha);
    off = writeVertex(dv, out, off, outPts[2][0], outPts[2][1], tint, alpha);
    off = writeVertex(dv, out, off, outPts[0][0], outPts[0][1], tint, alpha);
    off = writeVertex(dv, out, off, outPts[2][0], outPts[2][1], tint, alpha);
    off = writeVertex(dv, out, off, outPts[3][0], outPts[3][1], tint, alpha);
    return off;
}

G.__pixi_smoke_tick = function(dt) {
  const s = G.__pixi_smoke;
  if (!s) return;
  s.t += dt;
  s.frame = (s.frame + 1) | 0;
  const t = s.t;

    // Pixi scene-graph feature usage (Container + Sprite transforms) driving cmd-stream output.
    s.root.position.set(CX, CY);
    s.root.rotation = Math.sin(t * 0.32) * 0.28;
    s.root.scale.set(1.0 + Math.sin(t * 0.95) * 0.08);
    s.bg.rotation = t * 0.20;
    s.fg.rotation = -t * 0.27;

    const uiW = Math.max(420, Math.min(W - 80, W * 0.78));
    const uiH = Math.max(260, Math.min(H - 80, H * 0.70));
    s.uiRoot.position.set((W - uiW) * 0.5, (H - uiH) * 0.5);
    const uiBox = layoutUiBox(uiW, uiH);
    const uiX = s.uiRoot.position.x;
    const uiY = s.uiRoot.position.y;
    s.uiRoot.rotation = Math.sin(t * 0.22) * 0.02;
    s.cards[0].alpha = 0.16 + (Math.sin(t * 1.5) * 0.5 + 0.5) * 0.18;
    s.cards[1].alpha = 0.22 + (Math.sin(t * 1.3 + 1.1) * 0.5 + 0.5) * 0.22;
    s.cards[2].alpha = 0.16 + (Math.sin(t * 1.8 + 2.2) * 0.5 + 0.5) * 0.18;

  cmd.setViewport(W | 0, H | 0);
  cmd.setPremultipliedAlpha(false);
  cmd.setBlendMode(0);
  cmd.setBlendEnabled(true);
  cmd.setClearRgb(0xFFFFFF);
  cmd.beginFrame();

  let off = 0;
    off = emitSpriteQuadFromPixiNode(s.dv, s.out, off, s.uiRoot, s.panel);
    off = emitSpriteQuadFromPixiNode(s.dv, s.out, off, s.uiRoot, s.header);
    off = emitSpriteQuadFromPixiNode(s.dv, s.out, off, s.uiRoot, s.cards[0]);
    off = emitSpriteQuadFromPixiNode(s.dv, s.out, off, s.uiRoot, s.cards[1]);
    off = emitSpriteQuadFromPixiNode(s.dv, s.out, off, s.uiRoot, s.cards[2]);
    off = emitSpriteQuadFromPixiNode(s.dv, s.out, off, s.uiRoot, s.footer);

    // Core slabs from Pixi nodes (instead of raw hard-coded geometry).
    off = emitSpriteQuadFromPixiNode(s.dv, s.out, off, s.root, s.bg);
    off = emitSpriteQuadFromPixiNode(s.dv, s.out, off, s.root, s.fg);
  // Orbiting ring quads with varied alpha.
  for (let i = 0; i < RING_COUNT; i++) {
    const p = t * 0.7 + (i * (Math.PI * 2.0 / RING_COUNT));
    const rr = 86 + (12 * Math.sin(t * 0.9 + i * 0.73));
    const qx = CX + Math.cos(p) * rr;
    const qy = CY + Math.sin(p) * rr;
    const qa = (84 + ((Math.sin(t * 1.4 + i * 0.91) * 0.5 + 0.5) * 120)) | 0;
    const qw = 44 + (i % 3) * 10;
    const qh = 20 + (i % 4) * 6;
    const c = (i & 1) ? 0x2A84FF : 0xFF8B2E;
    off = emitQuad(
      s.dv, s.out, off,
      qx, qy,
      qw, qh,
      -p * 1.7,
      c,
      qa
    );
  }
  if (off > 0) {
    cmd.drawTrianglesU8(s.out.subarray(0, off));
  }

  // Additive glow accents.
  cmd.setBlendMode(1);
  let glowOff = 0;
  glowOff = emitQuad(
    s.dv, s.out, glowOff,
    CX + Math.cos(t * 0.6) * 42,
    CY + Math.sin(t * 0.5) * 34,
    180, 58,
    t * 1.1,
    0x7ACBFF,
    72
  );
  glowOff = emitQuad(
    s.dv, s.out, glowOff,
    CX + Math.cos(t * 0.4 + 1.7) * 50,
    CY + Math.sin(t * 0.7 + 0.8) * 26,
    170, 54,
    -t * 1.0,
    0xFFC07A,
    66
  );
  if (glowOff > 0) {
    cmd.drawTrianglesU8(s.out.subarray(0, glowOff));
  }
  cmd.setBlendMode(0);

    // Bottom parity proofs: fill, stroke-like edges, texture sampling, and blend mapping.
    const proofPad = 18;
    const proofGap = 12;
    const proofH = 94;
    const proofW = Math.max(150, ((W - proofPad * 2) - proofGap * 2) / 3);
    const proofY = H - proofH - 16;

    const p1x = proofPad + proofW * 0.5;
    const p2x = p1x + (proofW + proofGap);
    const p3x = p2x + (proofW + proofGap);
    const pcy = proofY + proofH * 0.5;

    let proofOff = 0;
    proofOff = emitQuad(s.dv, s.out, proofOff, p1x, pcy, proofW, proofH, 0.0, 0xF2F6FC, 255);
    proofOff = emitQuad(s.dv, s.out, proofOff, p2x, pcy, proofW, proofH, 0.0, 0xF2F6FC, 255);
    proofOff = emitQuad(s.dv, s.out, proofOff, p3x, pcy, proofW, proofH, 0.0, 0xF2F6FC, 255);

    // (1) Fill + alpha overlap.
    proofOff = emitQuad(s.dv, s.out, proofOff, p1x, pcy + 2, proofW - 28, proofH - 36, Math.sin(t * 0.9) * 0.08, 0x2A84FF, 155);
    proofOff = emitQuad(s.dv, s.out, proofOff, p1x + 8, pcy - 4, proofW - 46, proofH - 50, -Math.sin(t * 1.2 + 0.5) * 0.1, 0xFF8B2E, 180);

    // (2) Stroke-style edges from thin quads + rotating diagonal stroke.
    const strokeT = 3;
    const innerW = proofW - 30;
    const innerH = proofH - 36;
    const frameY = pcy + 1;
    proofOff = emitQuad(s.dv, s.out, proofOff, p2x, frameY - innerH * 0.5, innerW, strokeT, 0.0, 0x1E3D60, 240);
    proofOff = emitQuad(s.dv, s.out, proofOff, p2x, frameY + innerH * 0.5, innerW, strokeT, 0.0, 0x1E3D60, 240);
    proofOff = emitQuad(s.dv, s.out, proofOff, p2x - innerW * 0.5, frameY, strokeT, innerH, 0.0, 0x1E3D60, 240);
    proofOff = emitQuad(s.dv, s.out, proofOff, p2x + innerW * 0.5, frameY, strokeT, innerH, 0.0, 0x1E3D60, 240);
    proofOff = emitQuad(s.dv, s.out, proofOff, p2x, frameY, innerW - 10, 2, t * 0.7, 0xFF8B2E, 220);

    if (proofOff > 0) {
        cmd.drawTrianglesU8(s.out.subarray(0, proofOff));
    }

    if (s.proofTex) {
        // (3) Textured quad with clamp + linear sampling.
        cmd.setSampler(0, 0, 1, 1);
        let texOff = 0;
        texOff = emitTexturedQuad(
            s.texDv, s.texOut, texOff,
            p3x, pcy + 1,
            proofW - 30, proofH - 36,
            Math.sin(t * 0.6) * 0.07,
            0.0, 0.0, 1.0, 1.0,
            0xFFFFFF, 255
        );
        if (texOff > 0) {
            cmd.drawTexturedTrianglesU8(s.proofTex, s.texOut.subarray(0, texOff));
        }

        cmd.setBlendMode(0);
        cmd.setSampler(0, 0, 1, 1);
    }

  for (let i = 0; i < s.labels.length; i++) {
    const lb = s.labels[i];
    const a = t * 0.55 + lb.phase;
    const x = (CX + Math.cos(a) * lb.r - ((lb.text.length * 8) * 0.5)) | 0;
    const y = (CY + Math.sin(a) * lb.r - 6) | 0;
    cmd.drawAtlasText(
      s.atlasTex,
      1,
      x,
      y,
      String(lb.text || ''),
      Number(lb.size || 12),
      Number(lb.color || 0x101010),
      255
    );
  }

    let cursorOff = 0;
    for (let i = 0; i < s.globalCursors.length; i++) {
        const c = s.globalCursors[i];
        let st = s.cursorRuntime.get(c.id);
        if (!st) {
            const sx = W * c.posX;
            const sy = H * c.posY;
            st = { x: sx, y: sy, tx: sx, ty: sy, seen: false };
            s.cursorRuntime.set(c.id, st);
        }

        if (browserContext) {
            let bx = Number.NaN;
            let by = Number.NaN;
            try {
                bx = Number(browserContext.getCursorX ? browserContext.getCursorX(c.id) : Number.NaN);
                by = Number(browserContext.getCursorY ? browserContext.getCursorY(c.id) : Number.NaN);
            } catch {}

            let hovered = false;
            let focused = false;
            let menuOpen = false;
            try {
                hovered = !!(browserContext.getHoveredTarget && browserContext.getHoveredTarget(c.id));
                focused = !!(browserContext.getFocusedTarget && browserContext.getFocusedTarget(c.id));
                menuOpen = !!(browserContext.isContextMenuOpen && browserContext.isContextMenuOpen(c.id));
            } catch {}

            const hasPos = Number.isFinite(bx) && Number.isFinite(by);
            const hasSignal = hovered || focused || menuOpen || (hasPos && (bx !== 0 || by !== 0));
            if (hasPos && (hasSignal || st.seen)) {
                st.tx = bx;
                st.ty = by;
                st.seen = true;
            }
            s.cursorTilt.setState(c.id, hovered, focused || menuOpen);
        }

        const followK = Math.max(0.0, Math.min(1.0, dt * 18.0));
        st.x = st.x + (st.tx - st.x) * followK;
        st.y = st.y + (st.ty - st.y) * followK;
        const x = st.x;
        const y = st.y;
        const rot = s.cursorTilt.step(c.id, dt);
        cursorOff = emitCursorCross(s.dv, s.out, cursorOff, x, y, 10, 2, rot, c.color, 255);
    }

    // Dedicated animated AI cursor (ported from previous scene behavior).
    {
        const ai = s.aiCursor;
        const a = t * ai.speed + ai.phase;
        const cx = W * ai.centerX;
        const cy = H * ai.centerY;
        const x = cx + Math.cos(a) * ai.radius;
        const y = cy + Math.sin(a) * ai.radius;
        const rot = Math.sin(a * 1.7) * 0.35;
        cursorOff = emitCursorCross(s.dv, s.out, cursorOff, x, y, 10, 2, rot, ai.color, 255);
    }

    if (cursorOff > 0) {
        cmd.drawTrianglesU8(s.out.subarray(0, cursorOff));
    }

    let menuOff = 0;
    for (let i = 0; i < s.globalCursors.length; i++) {
        const c = s.globalCursors[i];
        const st = s.cursorRuntime.get(c.id);
        if (!st || !browserContext || !browserContext.isContextMenuOpen) continue;

        let isOpen = false;
        try {
            isOpen = !!browserContext.isContextMenuOpen(c.id);
        } catch {}
        if (!isOpen) continue;

        let menuX = 0;
        let menuY = 0;
        try {
            menuX = Number(browserContext.getContextMenuX ? browserContext.getContextMenuX(c.id) : 0);
            menuY = Number(browserContext.getContextMenuY ? browserContext.getContextMenuY(c.id) : 0);
        } catch {}

        const menuW = menuItemW + menuPad * 2;
        const menuH = menuLabels.length * menuItemH + menuPad * 2;
        menuX = Math.max(0, Math.min(W - menuW, menuX));
        menuY = Math.max(0, Math.min(H - menuH, menuY));

        menuOff = emitQuad(
            s.dv, s.out, menuOff,
            menuX + menuW * 0.5,
            menuY + menuH * 0.5,
            menuW,
            menuH,
            0.0,
            0xFFFFFF,
            255
        );

        // Owner-colored border for per-cursor context menu framing.
        menuOff = emitQuad(s.dv, s.out, menuOff, menuX + menuW * 0.5, menuY + menuBorderW * 0.5, menuW, menuBorderW, 0.0, c.color, 255);
        menuOff = emitQuad(s.dv, s.out, menuOff, menuX + menuW * 0.5, menuY + menuH - menuBorderW * 0.5, menuW, menuBorderW, 0.0, c.color, 255);
        menuOff = emitQuad(s.dv, s.out, menuOff, menuX + menuBorderW * 0.5, menuY + menuH * 0.5, menuBorderW, menuH, 0.0, c.color, 255);
        menuOff = emitQuad(s.dv, s.out, menuOff, menuX + menuW - menuBorderW * 0.5, menuY + menuH * 0.5, menuBorderW, menuH, 0.0, c.color, 255);

        let hoveredItem = -1;
        const px = Number(st.x || 0);
        const py = Number(st.y || 0);
        for (let item = 0; item < menuLabels.length; item++) {
            const rowX = menuX + menuPad;
            const rowY = menuY + menuPad + item * menuItemH;
            const rowHover = px >= rowX && px <= (rowX + menuItemW) && py >= rowY && py <= (rowY + menuItemH);
            if (rowHover) hoveredItem = item;
            menuOff = emitQuad(
                s.dv,
                s.out,
                menuOff,
                rowX + menuItemW * 0.5,
                rowY + menuItemH * 0.5,
                menuItemW,
                menuItemH,
                0.0,
                rowHover ? 0xF2F2F2 : 0xFFFFFF,
                255
            );
            cmd.drawAtlasText(
                s.atlasTex,
                1,
                (rowX + 8) | 0,
                (rowY + ((menuItemH - 12) * 0.5)) | 0,
                menuLabels[item],
                12,
                0x202020,
                255
            );
        }

        // Selecting any item closes the menu for the owner cursor.
        if (browserContext.getPointerDownSeq) {
            let seq = 0;
            let button = 0;
            try {
                seq = Number(browserContext.getPointerDownSeq(c.id) || 0) | 0;
                button = Number(browserContext.getPointerDownButton ? browserContext.getPointerDownButton(c.id) : 0) | 0;
            } catch {}
            const prevSeq = Number(s.menuClickSeq.get(c.id) || 0) | 0;
            if (seq !== prevSeq) {
                s.menuClickSeq.set(c.id, seq);
                if (hoveredItem >= 0 && button !== 2) {
                    const target =
                        (browserContext.getFocusedTarget && browserContext.getFocusedTarget(c.id))
                        || (browserContext.getContextMenuTarget && browserContext.getContextMenuTarget(c.id))
                        || (browserContext.getHoveredTarget && browserContext.getHoveredTarget(c.id))
                        || null;

                    if (hoveredItem === 0 && browserContext.setClipboardText && target != null) {
                        browserContext.setClipboardText(c.id, String(target));
                    } else if (hoveredItem === 1 && browserContext.getClipboardText) {
                        const clip = browserContext.getClipboardText(c.id) ?? '';
                        if (clip.length > 0 && browserContext.setClipboardText) {
                            browserContext.setClipboardText(c.id, clip);
                        }
                    }

                    if (browserContext.closeContextMenu) {
                        browserContext.closeContextMenu(c.id);
                    }
                }
            }
        }
    }
    if (menuOff > 0) {
        cmd.drawTrianglesU8(s.out.subarray(0, menuOff));
    }

    cmd.drawAtlasText(s.atlasTex, 1, (uiX + 32) | 0, (uiY + 26) | 0, 'Pixi Basic Layout', 16, 0x0F2A45, 255);
    cmd.drawAtlasText(s.atlasTex, 1, (uiX + 32) | 0, (uiY + 48) | 0, 'header / cards row / footer', 12, 0x214869, 230);
    cmd.drawAtlasText(s.atlasTex, 1, (uiX + 34) | 0, (uiY + uiBox.rowY + 16) | 0, 'card A', 12, 0x143658, 255);
    cmd.drawAtlasText(s.atlasTex, 1, (uiX + 34 + uiBox.cardW + 12) | 0, (uiY + uiBox.rowY + 16) | 0, 'card B', 12, 0x5A2A0D, 255);
    cmd.drawAtlasText(s.atlasTex, 1, (uiX + 34 + (uiBox.cardW + 12) * 2) | 0, (uiY + uiBox.rowY + 16) | 0, 'card C', 12, 0x143658, 255);
    cmd.drawAtlasText(s.atlasTex, 1, (uiX + 32) | 0, (uiY + uiBox.footerY + 18) | 0, 'footer', 12, 0x1A3247, 220);
    cmd.drawAtlasText(s.atlasTex, 1, (p1x - proofW * 0.5 + 10) | 0, (proofY + 10) | 0, 'fill + alpha', 11, 0x183A5B, 255);
    cmd.drawAtlasText(s.atlasTex, 1, (p2x - proofW * 0.5 + 10) | 0, (proofY + 10) | 0, 'stroke (thin quads)', 11, 0x183A5B, 255);
    cmd.drawAtlasText(s.atlasTex, 1, (p3x - proofW * 0.5 + 10) | 0, (proofY + 10) | 0, 'tex clamp+linear', 11, 0x183A5B, 255);

  cmd.endFrame();
};
"#;

        if !eval_or_log(
            ctx,
            init_script,
            init_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
            "init",
        ) {
            drop(vm);
            unsafe { trueos_cabi_gfx_present_owner_set(0) };
            PIXI_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
            return;
        }

        // Let module jobs/imports settle before ticks.
        for _ in 0..100 {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        let tick_filename = b"<pixi-smoke-tick>\0";
        let tick_script = b"var G=(typeof globalThis!=='undefined')?globalThis:this; if (G.__pixi_smoke_tick) G.__pixi_smoke_tick(0.05);";

        loop {
            if !eval_or_log(
                ctx,
                tick_script,
                tick_filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_GLOBAL,
                "tick",
            ) {
                break;
            }
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }

        qjs::workers::terminate_all_for_context(ctx);
        let _ = pump_runtime_once(rt, ctx);
        qjs::async_ops::drain_all_for_context(ctx);
        qjs::workers::drain_all_for_context(ctx);
        qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
        drop(vm);
    }

    unsafe { trueos_cabi_gfx_present_owner_set(0) };
    log_str("qjs-pixi-smoke: stopped\n");
    PIXI_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
}
