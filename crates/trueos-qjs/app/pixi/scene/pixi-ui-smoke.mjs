globalThis.__trueos_webgl_force = 1;

if (!globalThis.HTMLCanvasElement || !globalThis.HTMLCanvasElement.__trueosPatched) {
  const Ctor = function HTMLCanvasElement() {};
  Object.defineProperty(Ctor, '__trueosPatched', { value: 1 });
  Object.defineProperty(Ctor, Symbol.hasInstance, {
    value(obj) {
      return !!obj && typeof obj.getContext === 'function' && 'width' in obj && 'height' in obj;
    },
  });
  globalThis.HTMLCanvasElement = Ctor;
}
if (!globalThis.CanvasRenderingContext2D || !globalThis.CanvasRenderingContext2D.__trueosPatched) {
  const Ctor = function CanvasRenderingContext2D() {};
  Object.defineProperty(Ctor, '__trueosPatched', { value: 1 });
  Object.defineProperty(Ctor, Symbol.hasInstance, {
    value(obj) {
      return !!obj && typeof obj.fillRect === 'function';
    },
  });
  globalThis.CanvasRenderingContext2D = Ctor;
}

const PIXI = await import('pixi.js@7.4.0?bundle&target=es2022');
const processMod = await import('node:process');
const process = processMod.default || processMod;

const log = globalThis.print;
log('pixi-ui: start');

function nowSeconds() {
  const t = process.hrtime();
  return t[0] + (t[1] / 1e9);
}

function clampByte(v) {
  if (v < 0) return 0;
  if (v > 255) return 255;
  return v | 0;
}

function mixRgb(a, b, t) {
  return [
    clampByte(a[0] + (b[0] - a[0]) * t),
    clampByte(a[1] + (b[1] - a[1]) * t),
    clampByte(a[2] + (b[2] - a[2]) * t),
  ];
}

function rgbToHex(rgb) {
  return ((rgb[0] & 255) << 16) | ((rgb[1] & 255) << 8) | (rgb[2] & 255);
}

function drawButtonSkin(g, w, h, state) {
  const base = [58, 70, 86];
  const edge = [95, 120, 145];
  const hi = [120, 160, 210];
  const lo = [38, 48, 60];

  let fill = base;
  let border = edge;
  let alpha = 1.0;
  let inset = 0;

  if (state === 'hovered') {
    fill = mixRgb(base, hi, 0.45);
    border = mixRgb(edge, hi, 0.65);
  } else if (state === 'pressed') {
    fill = mixRgb(base, lo, 0.55);
    border = mixRgb(edge, lo, 0.2);
    inset = 1;
  } else if (state === 'disabled') {
    fill = mixRgb(base, lo, 0.7);
    border = mixRgb(edge, lo, 0.75);
    alpha = 0.55;
  }

  const x = inset;
  const y = inset;
  const bw = w - inset * 2;
  const bh = h - inset * 2;

  g.clear();
  g.alpha = alpha;
  g.lineStyle(2, rgbToHex(border), 1.0);
  g.beginFill(rgbToHex(fill), 1.0);
  g.drawRoundedRect(x, y, bw, bh, 12);
  g.endFill();

  const shine = mixRgb(fill, [220, 235, 255], 0.15);
  g.lineStyle(1, rgbToHex(shine), 0.85);
  g.moveTo(x + 10, y + 10);
  g.lineTo(x + bw - 10, y + 10);
}

function drawGlyph(g, w, h, state) {
  const on = state === 'disabled' ? [95, 108, 124] : [210, 226, 244];
  const accent = state === 'pressed' ? [240, 170, 120] : [120, 200, 255];

  g.clear();
  g.lineStyle(3, rgbToHex(on), 1.0);
  g.moveTo(10, h * 0.5);
  g.lineTo(w - 12, h * 0.5);
  g.lineStyle(2, rgbToHex(accent), 0.95);
  g.moveTo(w * 0.35, h * 0.33);
  g.lineTo(w * 0.5, h * 0.68);
  g.lineTo(w * 0.65, h * 0.33);
}

function makeButton(x, y, w, h, state) {
  const c = new PIXI.Container();
  c.position.set(x, y);

  const skin = new PIXI.Graphics();
  const glyph = new PIXI.Graphics();
  c.addChild(skin);
  c.addChild(glyph);

  drawButtonSkin(skin, w, h, state);
  drawGlyph(glyph, w, h, state);

  return { container: c, state };
}

try {
  const width = 640;
  const height = 420;

  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;

  const gl = canvas.getContext('webgl');
  log('pixi-ui: gl ctx', gl ? 'ok' : 'null');
  if (!gl) throw new Error('no webgl context');

  const renderer = new PIXI.Renderer({
    view: canvas,
    context: gl,
    width,
    height,
    antialias: false,
    backgroundColor: 0x12161D,
  });
  log('pixi-ui: renderer', renderer && renderer.type !== undefined ? String(renderer.type) : 'unknown');

  const stage = new PIXI.Container();

  const root = new PIXI.Container();
  root.position.set(88, 46);
  stage.addChild(root);

  const frame = new PIXI.Graphics();
  frame.lineStyle(2, 0x3A485A, 1.0);
  frame.beginFill(0x1A222D, 0.92);
  frame.drawRoundedRect(0, 0, 464, 300, 20);
  frame.endFill();
  root.addChild(frame);

  const toolbar = new PIXI.Container();
  toolbar.position.set(18, 16);
  root.addChild(toolbar);

  const toolbarBg = new PIXI.Graphics();
  toolbarBg.beginFill(0x10151D, 0.9);
  toolbarBg.lineStyle(1, 0x304255, 1.0);
  toolbarBg.drawRoundedRect(0, 0, 428, 48, 12);
  toolbarBg.endFill();
  toolbar.addChild(toolbarBg);

  const toolbarLine = new PIXI.Graphics();
  toolbarLine.lineStyle(2, 0x5EA6E8, 0.9);
  toolbarLine.moveTo(18, 24);
  toolbarLine.lineTo(170, 24);
  toolbarLine.lineStyle(2, 0xF0A878, 0.9);
  toolbarLine.moveTo(258, 24);
  toolbarLine.lineTo(410, 24);
  toolbar.addChild(toolbarLine);

  const btnGroup = new PIXI.Container();
  btnGroup.position.set(28, 88);
  root.addChild(btnGroup);

  const buttons = [
    makeButton(0, 0, 130, 74, 'normal'),
    makeButton(146, 0, 130, 74, 'hovered'),
    makeButton(292, 0, 130, 74, 'disabled'),
    makeButton(73, 96, 130, 74, 'pressed'),
    makeButton(219, 96, 130, 74, 'normal'),
  ];

  for (let i = 0; i < buttons.length; i++) {
    btnGroup.addChild(buttons[i].container);
  }

  let phase = 0.0;
  let frames = 0;
  let lastFpsSec = nowSeconds();
  while (true) {
    phase += 0.04;

    root.rotation = 0.015 * Math.sin(phase * 0.25);
    root.scale.set(1.0 + 0.01 * Math.sin(phase * 0.5));

    for (let i = 0; i < buttons.length; i++) {
      const b = buttons[i];

      if (b.state === 'hovered') {
        b.container.y = (i >= 3 ? 96 : 0) + 1.8 * Math.sin(phase * 1.1 + i);
        b.container.scale.set(1.0 + 0.03 * Math.sin(phase * 1.25 + i));
        b.container.alpha = 0.92 + 0.08 * (0.5 + 0.5 * Math.sin(phase * 1.15 + i));
      } else if (b.state === 'pressed') {
        b.container.y = (i >= 3 ? 96 : 0) + 2.0 + 1.2 * Math.sin(phase * 1.8);
        b.container.scale.set(0.985 + 0.01 * Math.sin(phase * 1.4));
        b.container.alpha = 1.0;
      } else {
        b.container.y = (i >= 3 ? 96 : 0);
        b.container.scale.set(1.0, 1.0);
        b.container.alpha = b.state === 'disabled' ? 0.58 : 1.0;
      }
    }

    renderer.render(stage);
    frames++;

    const now = nowSeconds();
    const dt = now - lastFpsSec;
    if (dt >= 15.0) {
      const fps = Math.floor(frames / dt);
      log('pixi-ui: fps-est', String(fps), 'dt', String(dt));
      frames = 0;
      lastFpsSec = now;
    }
  }
} catch (e) {
  log('pixi-ui: error', (e && e.stack) ? e.stack : (e && e.message) ? e.message : String(e));
}

export {};
