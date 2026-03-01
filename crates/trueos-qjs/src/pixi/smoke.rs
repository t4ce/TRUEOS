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
const PIXI = await import('/qjs/vendor/pixi.mjs');
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
footer.tint = 0x1B3D63;
footer.alpha = 0.14;
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
  atlasTex,
    proofTex,
  out,
  dv,
    texOut,
    texDv,
  t: 0.0,
  frame: 0,
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

    const uiX = s.uiRoot.position.x;
    const uiY = s.uiRoot.position.y;
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
