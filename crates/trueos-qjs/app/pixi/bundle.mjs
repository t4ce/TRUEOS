import * as PIXI from 'pixi.js@7.4.0?bundle&target=es2022';

// Allow getContext('webgl') one-arg calls during this smoke.
globalThis.__trueos_webgl_force = 1;

function createTriangleScene() {
  // Triangle in pixel coords.
  const positions = new Float32Array([
    40, 40,
    280, 60,
    160, 170,
  ]);

  // Per-vertex RGBA (normalized u8): R, G, B corners.
  const colors = new Uint8Array([
    255, 0, 0, 255,
    0, 255, 0, 255,
    0, 0, 255, 255,
  ]);

  const indices = new Uint16Array([0, 1, 2]);

  const geometry = new PIXI.Geometry()
    .addAttribute('aVertexPosition', positions, 2, false, PIXI.TYPES.FLOAT)
    .addAttribute('aColor', colors, 4, true, PIXI.TYPES.UNSIGNED_BYTE)
    .addIndex(indices);

  const vs = `precision mediump float;
attribute vec2 aVertexPosition;
attribute vec4 aColor;
uniform mat3 translationMatrix;
uniform mat3 projectionMatrix;
varying vec4 vColor;
void main(){
    vColor = aColor;
    vec3 pos = projectionMatrix * translationMatrix * vec3(aVertexPosition, 1.0);
    gl_Position = vec4(pos.xy, 0.0, 1.0);
}`;

  const fs = `precision mediump float;
varying vec4 vColor;
void main(){
    gl_FragColor = vColor;
}`;

  const program = PIXI.Program.from(vs, fs);
  const shader = new PIXI.Shader(program, {});
  const mesh = new PIXI.Mesh(geometry, shader);
  mesh.position.set(160, 100);
  mesh.pivot.set(160, 100);

  const stage = new PIXI.Container();
  stage.addChild(mesh);
  return { stage, mesh };
}

const log = globalThis.print;
log('pixi-tri: start');

try {
  const canvas = document.createElement('canvas');
  canvas.width = 320;
  canvas.height = 200;

  const gl = canvas.getContext('webgl');
  log('pixi-tri: gl ctx', gl ? 'ok' : 'null');
  if (!gl) throw new Error('no webgl context');
  log(
    'pixi-tri: gl attrs',
    (gl.getContextAttributes && gl.getContextAttributes)
      ? JSON.stringify(gl.getContextAttributes())
      : 'missing',
  );
  log(
    'pixi-tri: gl ext oes_uint32',
    (gl.getExtension && gl.getExtension)
      ? String(!!gl.getExtension('OES_element_index_uint'))
      : 'missing',
  );

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

  const { stage, mesh } = createTriangleScene();

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
