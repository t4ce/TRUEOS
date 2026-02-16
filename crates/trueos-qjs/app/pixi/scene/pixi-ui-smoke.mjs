import * as PIXI from 'pixi.js@7.4.0';
import processMod from 'node:process';

const process = processMod.default || processMod;
const log = globalThis.print;

const VIEW_W = 1280;
const VIEW_H = 800;

function nowSeconds() {
  const t = process.hrtime();
  return t[0] + (t[1] / 1e9);
}

log('pixi-ui: start');

try {
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
    backgroundColor: 0x12161d,
    clearBeforeRender: true,
    powerPreference: 'high-performance',
  });

  const stage = new PIXI.Container();
  log('pixi-ui: renderer', String(renderer.type));

  const root = new PIXI.Container();
  root.position.set(VIEW_W * 0.5, VIEW_H * 0.5);
  stage.addChild(root);

  const shell = new PIXI.Graphics();
  shell.beginFill(0x2b3242, 1.0);
  shell.drawRoundedRect(-240, -160, 480, 320, 36);
  shell.endFill();
  root.addChild(shell);

  const row = new PIXI.Container();
  row.position.set(-180, -20);
  root.addChild(row);

  for (let i = 0; i < 4; i++) {
    const btn = new PIXI.Graphics();
    btn.beginFill(0xd1853a, 1.0);
    btn.drawRoundedRect(i * 95, 0, 80, 72, 16);
    btn.endFill();
    row.addChild(btn);
  }

  let t = 0.0;
  let reportStart = nowSeconds();
  let frames = 0;
  while (true) {
    t += 0.022;
    root.rotation = Math.sin(t) * 0.15;
    root.scale.set(1.0 + Math.sin(t * 0.7) * 0.03);

    renderer.render(stage);
    frames++;

    const now = nowSeconds();
    const dt = now - reportStart;
    if (dt >= 15.0) {
      const fps = frames / dt;
      log('pixi-ui: fps-est', String(Math.floor(fps)), 'dt', String(dt));
      reportStart = now;
      frames = 0;
    }
  }
} catch (e) {
  log('pixi-ui: error', (e && e.stack) ? e.stack : (e && e.message) ? e.message : String(e));
}

export {};
