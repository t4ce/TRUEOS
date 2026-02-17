import * as PIXI from 'pixi.js@7.4.0';
import processMod from 'node:process';
import { getFontAtlasSmall, getFontAtlasLarge } from 'trueos:text';

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
  const textLayer = new PIXI.Container();
  stage.addChild(textLayer);

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

  function buildFontAtlas(atlas) {
    const pixels = new Uint8Array(atlas.pixels);
    const base = PIXI.BaseTexture.fromBuffer(pixels, atlas.width, atlas.height);
    const index = new Uint16Array(atlas.index);
    const widths = atlas.widths ? new Uint8Array(atlas.widths) : null;
    const textures = [];
    const total = atlas.gridW * atlas.gridH;
    for (let i = 0; i < total; i++) {
      const col = i % atlas.gridW;
      const row = (i / atlas.gridW) | 0;
      const rect = new PIXI.Rectangle(
        col * atlas.cellW,
        row * atlas.cellH,
        atlas.cellW,
        atlas.cellH,
      );
      textures.push(new PIXI.Texture(base, rect));
    }
    return {
      textures,
      index,
      widths,
      cellW: atlas.cellW,
      cellH: atlas.cellH,
      gridW: atlas.gridW,
      gridH: atlas.gridH,
    };
  }

  function drawTextLine(text, x, y, color, font) {
    const cont = new PIXI.Container();
    let penX = x;
    const idxQ = font.index['?'.charCodeAt(0)] || 0;
    for (let i = 0; i < text.length; i++) {
      const code = text.charCodeAt(i);
      let slot = font.index[code];
      if (slot === 0xFFFF || slot === undefined) slot = idxQ;
      const tex = font.textures[slot];
      const spr = new PIXI.Sprite(tex);
      spr.tint = color;
      spr.position.set(penX, y);
      cont.addChild(spr);
      let adv = font.cellW;
      if (font.widths && slot < font.widths.length) {
        const w = font.widths[slot] | 0;
        if (w > 0 && w <= font.cellW) adv = w;
      }
      penX += adv + 1;
    }
    return cont;
  }

  try {
    const small = buildFontAtlas(getFontAtlasSmall());
    const large = buildFontAtlas(getFontAtlasLarge());

    textLayer.addChild(drawTextLine('TRUEOS UI', 230, 120, 0xE8F2FF, large));
    textLayer.addChild(drawTextLine('STATUS', 230, 205, 0xCBE3FF, small));
    textLayer.addChild(drawTextLine('PANEL', 460, 205, 0xCBE3FF, small));

    textLayer.addChild(drawTextLine('OK', 478, 638, 0x112021, small));
    textLayer.addChild(drawTextLine('CANCEL', 602, 638, 0xECF6FF, small));
    textLayer.addChild(drawTextLine('APPLY', 726, 638, 0x112021, small));
  } catch (e) {
    log('pixi-ui: text', 'off', (e && e.message) ? e.message : String(e));
  }

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

  log('pixi-ui: worker', 'off', 'removed');

  function renderStep() {
    t += 0.016;
    ui.alpha = 0.93 + Math.sin(t * 1.1) * 0.02;

    // Pseudo mouse movement: smooth looping path across the panel.
    const cx = 640 + Math.sin(t * 0.95) * 360 + Math.sin(t * 2.1) * 30;
    const cy = 400 + Math.cos(t * 0.73) * 230 + Math.sin(t * 1.7) * 20;
    const hovered = findHovered(cx, cy);
    if (hovered) {
      hoverStrength = 0.35 + (Math.sin(t * 3.2) * 0.25);
    } else {
      hoverStrength = 0.0;
    }
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
      );
      reportStart = now;
      frames = 0;
    }
  }

  function scheduleTick() {
    const step = () => {
      for (let i = 0; i < FRAMES_PER_TICK; i++) {
        renderStep();
      }
    };
    if (typeof globalThis.requestAnimationFrame === 'function') {
      const rafLoop = () => {
        step();
        globalThis.requestAnimationFrame(rafLoop);
      };
      globalThis.requestAnimationFrame(rafLoop);
      return;
    }
    if (typeof globalThis.setTimeout === 'function') {
      const toLoop = () => {
        step();
        globalThis.setTimeout(toLoop, 0);
      };
      globalThis.setTimeout(toLoop, 0);
      return;
    }
    const ntLoop = () => {
      step();
      process.nextTick(ntLoop);
    };
    process.nextTick(ntLoop);
  }

  scheduleTick();
} catch (e) {
  log('pixi-ui: error', (e && e.stack) ? e.stack : (e && e.message) ? e.message : String(e));
}

export {};
