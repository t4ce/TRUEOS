import * as PIXI from 'pixi.js@7.4.0';
import processMod from 'node:process';

const process = processMod.default || processMod;
const log = globalThis.print;

const VIEW_W = 1280;
const VIEW_H = 800;
const FRAMES_PER_TICK = 1;
const FPS_REPORT_SEC = 5.0;

function nowSeconds() {
  const t = process.hrtime();
  return t[0] + (t[1] / 1e9);
}

log('pixi-ui: start');
log('pixi-ui: scene', 'reintro-ui-v1');

try {
  let workerPongs = 0;
  let workerPosts = 0;
  let worker = null;
  let workerReady = false;
  let workerReqInFlight = false;
  let hoverIndex = -1;
  let hoverStrength = 0.0;

  const canvas = document.createElement('canvas');
  canvas.width = VIEW_W;
  canvas.height = VIEW_H;
  if (document.body && document.body.appendChild) {
    document.body.appendChild(canvas);
  }

  const gl = canvas.getContext('webgl', {
    antialias: false,
    alpha: false,
    depth: false,
    stencil: false,
    preserveDrawingBuffer: false,
  });
  log('pixi-ui: gl ctx', gl ? 'ok' : 'null');

  const renderer = new PIXI.Renderer({
    view: canvas,
    context: gl,
    width: VIEW_W,
    height: VIEW_H,
    antialias: false,
    autoDensity: false,
    backgroundAlpha: 1,
    backgroundColor: 0x101820,
    clearBeforeRender: true,
    powerPreference: 'high-performance',
  });

  const stage = new PIXI.Container();
  log('pixi-ui: renderer', String(renderer.type));

  const ui = new PIXI.Graphics();
  stage.addChild(ui);
  const hoverFx = new PIXI.Container();
  stage.addChild(hoverFx);
  const cursor = new PIXI.Container();
  stage.addChild(cursor);
  const hitRects = [];

  // Retained hover visuals: created once, moved/scaled/tinted on demand.
  const hoverGlow = new PIXI.Graphics();
  hoverGlow.beginFill(0x9fd3ff, 0.16);
  hoverGlow.drawRect(0, 0, 1, 1);
  hoverGlow.endFill();
  hoverGlow.visible = false;
  hoverFx.addChild(hoverGlow);

  const hoverCore = new PIXI.Graphics();
  hoverCore.beginFill(0xffffff, 0.22);
  hoverCore.drawRect(0, 0, 1, 1);
  hoverCore.endFill();
  hoverCore.visible = false;
  hoverFx.addChild(hoverCore);

  const cursorHalo = new PIXI.Graphics();
  cursorHalo.beginFill(0xffffff, 0.12);
  cursorHalo.drawRect(-10, -10, 20, 20);
  cursorHalo.endFill();
  cursorHalo.visible = false;
  hoverFx.addChild(cursorHalo);

  // Flat, low-complexity UI reintroduction geometry.
  ui.beginFill(0x233142, 1.0);
  ui.drawRect(180, 90, 920, 620);
  ui.endFill();

  ui.beginFill(0x2f455f, 1.0);
  ui.drawRect(200, 110, 880, 70);
  ui.endFill();

  ui.beginFill(0x1a2533, 1.0);
  ui.drawRect(200, 200, 200, 490);
  ui.endFill();

  ui.beginFill(0x1e2d3f, 1.0);
  ui.drawRect(420, 200, 660, 490);
  ui.endFill();

  for (let i = 0; i < 6; i++) {
    const x = 230;
    const y = 230 + i * 74;
    ui.beginFill(i === 1 ? 0x79b8ff : 0x425b78, 1.0);
    ui.drawRect(x, y, 140, 44);
    ui.endFill();
    hitRects.push({ kind: 'pill', x, y, w: 140, h: 44 });
  }

  for (let r = 0; r < 2; r++) {
    for (let c = 0; c < 2; c++) {
      const x = 460 + c * 320;
      const y = 230 + r * 190;

      ui.beginFill(0x5f82a8, 1.0);
      ui.drawRect(x, y, 280, 150);
      ui.endFill();
      hitRects.push({ kind: 'card', x, y, w: 280, h: 150 });

      ui.beginFill(0xaad4ff, 1.0);
      ui.drawRect(x + 20, y + 20, 160, 14);
      ui.endFill();

      ui.beginFill(0x35506d, 1.0);
      ui.drawRect(x + 20, y + 48, 220, 10);
      ui.drawRect(x + 20, y + 66, 200, 10);
      ui.drawRect(x + 20, y + 84, 178, 10);
      ui.endFill();
    }
  }

  for (let i = 0; i < 5; i++) {
    const x = 460 + i * 124;
    const y = 630;
    ui.beginFill(i === 2 ? 0x6ee7a8 : 0x4e6a88, 1.0);
    ui.drawRect(x, y, 108, 36);
    ui.endFill();
    hitRects.push({ kind: 'button', x, y, w: 108, h: 36 });
  }

  const cursorBody = new PIXI.Graphics();
  cursorBody.beginFill(0xf6f7fb, 1.0);
  cursorBody.drawPolygon([
    0, 0,
    0, 26,
    8, 20,
    13, 34,
    19, 31,
    14, 17,
    26, 17,
  ]);
  cursorBody.endFill();
  cursor.addChild(cursorBody);

  const cursorStroke = new PIXI.Graphics();
  cursorStroke.beginFill(0x17212f, 1.0);
  cursorStroke.drawRect(-1, -1, 2, 28);
  cursorStroke.drawRect(-1, -1, 20, 2);
  cursorStroke.endFill();
  cursor.addChild(cursorStroke);

  let t = 0.0;
  let reportStart = nowSeconds();
  let frames = 0;

  function findHovered(x, y) {
    for (let i = 0; i < hitRects.length; i++) {
      const r = hitRects[i];
      if (x >= r.x && y >= r.y && x < (r.x + r.w) && y < (r.y + r.h)) {
        return r;
      }
    }
    return null;
  }

  function drawHoverFx(hovered, x, y) {
    if (!hovered) {
      hoverGlow.visible = false;
      hoverCore.visible = false;
      cursorHalo.visible = false;
      return;
    }
    hoverGlow.visible = true;
    hoverCore.visible = true;
    cursorHalo.visible = true;

    hoverGlow.position.set(hovered.x - 6, hovered.y - 6);
    hoverGlow.width = hovered.w + 12;
    hoverGlow.height = hovered.h + 12;
    hoverGlow.alpha = 0.10 + (hoverStrength * 0.28);

    hoverCore.position.set(hovered.x, hovered.y);
    hoverCore.width = hovered.w;
    hoverCore.height = hovered.h;
    hoverCore.tint = hovered.kind === 'button' ? 0x8cffc7 : 0xc0e6ff;
    hoverCore.alpha = 0.12 + (hoverStrength * 0.30);

    cursorHalo.position.set(x, y);
    cursorHalo.alpha = 0.08 + (hoverStrength * 0.20);
  }

  try {
    worker = new Worker(`
      import { parentPort } from 'node:worker_threads';

      let rects = [];
      function edgeDistance(x, y, r) {
        const dx = Math.max(r.x - x, 0, x - (r.x + r.w));
        const dy = Math.max(r.y - y, 0, y - (r.y + r.h));
        return Math.sqrt(dx * dx + dy * dy);
      }

      parentPort.onMessage((raw) => {
        let msg = null;
        try { msg = JSON.parse(String(raw || '{}')); } catch (_) {}
        if (!msg || !msg.cmd) {
          parentPort.postMessage('{"ok":0}');
          return;
        }
        if (msg.cmd === 'init') {
          rects = Array.isArray(msg.rects) ? msg.rects : [];
          parentPort.postMessage('{"ok":1,"ready":1}');
          return;
        }
        if (msg.cmd === 'sample') {
          const x = Number(msg.x || 0);
          const y = Number(msg.y || 0);
          const t = Number(msg.t || 0);
          let bestI = -1;
          let bestV = 0.0;
          // Synthetic heavier compute slice (distance + oscillation field).
          for (let i = 0; i < rects.length; i++) {
            const r = rects[i];
            const d = edgeDistance(x, y, r);
            const base = Math.exp(-d / 90.0);
            const wobble = 0.5 + 0.5 * Math.sin((i * 0.43) + (t * 2.7));
            const v = base * (0.7 + 0.3 * wobble);
            if (v > bestV) {
              bestV = v;
              bestI = i;
            }
          }
          parentPort.postMessage(JSON.stringify({ ok: 1, hoverIndex: bestI, strength: bestV }));
          return;
        }
        parentPort.postMessage('{"ok":0}');
      });
    `);

    const onWorkerMessage = (raw) => {
      workerReqInFlight = false;
      workerPongs++;
      let msg = null;
      try { msg = JSON.parse(String(raw || '{}')); } catch (_) {}
      if (!msg || msg.ok !== 1) return;
      if (msg.ready === 1) {
        workerReady = true;
        return;
      }
      if (Number.isFinite(msg.hoverIndex)) hoverIndex = msg.hoverIndex | 0;
      if (Number.isFinite(msg.strength)) hoverStrength = Math.max(0, Math.min(1, Number(msg.strength)));
    };

    if (typeof worker.onMessage === 'function') {
      worker.onMessage(onWorkerMessage);
    } else if (typeof worker.addEventListener === 'function') {
      worker.addEventListener('message', onWorkerMessage);
    }

    const rectsForWorker = hitRects.map((r) => ({ x: r.x, y: r.y, w: r.w, h: r.h }));
    worker.postMessage(JSON.stringify({ cmd: 'init', rects: rectsForWorker }));
    workerPosts++;
    log('pixi-ui: worker', 'ok');
  } catch (e) {
    log('pixi-ui: worker', 'off', (e && e.message) ? e.message : String(e));
  }

  function renderStep() {
    t += 0.016;
    ui.alpha = 0.93 + Math.sin(t * 1.1) * 0.02;

    // Pseudo mouse movement: smooth looping path across the panel.
    const cx = 640 + Math.sin(t * 0.95) * 360 + Math.sin(t * 2.1) * 30;
    const cy = 400 + Math.cos(t * 0.73) * 230 + Math.sin(t * 1.7) * 20;
    if (worker && workerReady && !workerReqInFlight && ((frames & 1) === 0)) {
      workerReqInFlight = true;
      workerPosts++;
      worker.postMessage(JSON.stringify({ cmd: 'sample', x: cx, y: cy, t }));
    }
    const hovered = (hoverIndex >= 0 && hoverIndex < hitRects.length)
      ? hitRects[hoverIndex]
      : findHovered(cx, cy);
    if (!hovered) hoverStrength = 0.0;
    drawHoverFx(hovered, cx, cy);
    cursor.position.set(cx | 0, cy | 0);

    renderer.render(stage);
    frames++;

    const now = nowSeconds();
    const dt = now - reportStart;
    if (dt >= FPS_REPORT_SEC) {
      const fps = frames / dt;
      log(
        'pixi-ui: fps-est',
        String(Math.floor(fps)),
        'dt',
        String(dt),
        'wp',
        String(workerPosts),
        'wr',
        String(workerPongs),
      );
      reportStart = now;
      frames = 0;
    }
  }

  function scheduleTick() {
    process.nextTick(() => {
      for (let i = 0; i < FRAMES_PER_TICK; i++) {
        renderStep();
      }
      scheduleTick();
    });
  }

  scheduleTick();
} catch (e) {
  log('pixi-ui: error', (e && e.stack) ? e.stack : (e && e.message) ? e.message : String(e));
}

export {};
