import * as PIXI from 'pixi.js@7.4.0?bundle&target=es2022';
import { createTriangleScene } from './scene/triangle.mjs';

// Allow getContext('webgl') one-arg calls during this smoke.
globalThis.__trueos_webgl_force = 1;

const log = globalThis.print;
log('pixi-tri: start');

try {
  const canvas = document.createElement('canvas');
  canvas.width = 320;
  canvas.height = 200;

  const gl = canvas.getContext('webgl');
  log('pixi-tri: gl ctx', gl ? 'ok' : 'null');
  if (!gl) throw new Error('no webgl context');

  const renderer = new PIXI.Renderer({
    view: canvas,
    context: gl,
    width: canvas.width,
    height: canvas.height,
    antialias: false,
    backgroundColor: 0x081830,
  });
  log(
    'pixi-tri: renderer',
    renderer && renderer.type !== undefined ? String(renderer.type) : 'unknown',
  );

  const { stage, mesh } = createTriangleScene(PIXI);

  // Drive a short in-VM animation burst; our shim doesn't provide browser RAF/timers yet.
  for (let i = 0; i < 120; i++) {
    mesh.rotation += 0.04;
    renderer.render(stage);
  }

  log('pixi-tri: ok');
} catch (e) {
  log('pixi-tri: error', (e && e.stack) ? e.stack : (e && e.message) ? e.message : String(e));
}

export {};
