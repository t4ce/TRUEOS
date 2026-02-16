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
log('pixi-ui: scene', 'probe-rgb-mesh-v1');

try {
  const workerPongs = 0;
  const workerPosts = 0;
  const workerScaleBias = 0.0;

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
    backgroundColor: 0x0f1e34,
    clearBeforeRender: true,
    powerPreference: 'high-performance',
  });

  const stage = new PIXI.Container();
  log('pixi-ui: renderer', String(renderer.type));

  // Probe geometry: one triangle with per-vertex RGB to force linear interpolation.
  const vertices = new Float32Array([
    640, 140,  // top
    210, 660,  // left
    1070, 660, // right
  ]);
  const colors = new Float32Array([
    1.0, 0.0, 0.0, // red
    0.0, 1.0, 0.0, // green
    0.0, 0.0, 1.0, // blue
  ]);
  const indices = new Uint16Array([0, 1, 2]);

  const geometry = new PIXI.Geometry()
    .addAttribute('aVertexPosition', vertices, 2)
    .addAttribute('aColor', colors, 3)
    .addIndex(indices);

  const vertexSrc = `
    precision mediump float;
    attribute vec2 aVertexPosition;
    attribute vec3 aColor;
    uniform mat3 translationMatrix;
    uniform mat3 projectionMatrix;
    varying vec3 vColor;
    void main(void) {
      vColor = aColor;
      vec3 pos = projectionMatrix * translationMatrix * vec3(aVertexPosition, 1.0);
      gl_Position = vec4(pos.xy, 0.0, 1.0);
    }
  `;
  const fragmentSrc = `
    precision mediump float;
    varying vec3 vColor;
    void main(void) {
      gl_FragColor = vec4(vColor, 1.0);
    }
  `;

  const shader = PIXI.Shader.from(vertexSrc, fragmentSrc);
  const tri = new PIXI.Mesh(geometry, shader);
  stage.addChild(tri);

  let t = 0.0;
  let reportStart = nowSeconds();
  let frames = 0;
  let totalFrames = 0;

  function renderStep() {
    t += 0.016;
    const pulse = Math.sin(t * 1.25) * (0.03 + workerScaleBias);
    tri.alpha = 0.95 + pulse * 0.03;

    renderer.render(stage);
    frames++;
    totalFrames++;

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

  log('pixi-ui: worker', 'off');

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
