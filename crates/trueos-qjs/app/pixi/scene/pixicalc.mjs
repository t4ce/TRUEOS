import * as PIXI from 'pixi.js@7.4.0?bundle&target=es2022';

// Allow getContext('webgl') one-arg calls during this smoke.
globalThis.__trueos_webgl_force = 1;

const log = globalThis.print;
log('pixi-calc: start');

function mkProgram(PIXI) {
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

  return PIXI.Program.from(vs, fs);
}

function mkRectMesh(PIXI, program, x, y, w, h, rgb) {
  const positions = new Float32Array([
    x, y,
    x + w, y,
    x + w, y + h,
    x, y + h,
  ]);
  const [r, g, b] = rgb;
  const colors = new Uint8Array([
    r, g, b, 255,
    r, g, b, 255,
    r, g, b, 255,
    r, g, b, 255,
  ]);
  const indices = new Uint16Array([0, 1, 2, 0, 2, 3]);
  const geometry = new PIXI.Geometry()
    .addAttribute('aVertexPosition', positions, 2, false, PIXI.TYPES.FLOAT)
    .addAttribute('aColor', colors, 4, true, PIXI.TYPES.UNSIGNED_BYTE)
    .addIndex(indices);
  return new PIXI.Mesh(geometry, new PIXI.Shader(program, {}));
}

function mkTriangleMesh(PIXI, program, pts, rgb) {
  const positions = new Float32Array(pts);
  const [r, g, b] = rgb;
  const colors = new Uint8Array([
    r, g, b, 255,
    r, g, b, 255,
    r, g, b, 255,
  ]);
  const indices = new Uint16Array([0, 1, 2]);
  const geometry = new PIXI.Geometry()
    .addAttribute('aVertexPosition', positions, 2, false, PIXI.TYPES.FLOAT)
    .addAttribute('aColor', colors, 4, true, PIXI.TYPES.UNSIGNED_BYTE)
    .addIndex(indices);
  return { mesh: new PIXI.Mesh(geometry, new PIXI.Shader(program, {})), geometry };
}

try {
  const canvas = document.createElement('canvas');
  canvas.width = 640;
  canvas.height = 420;

  const gl = canvas.getContext('webgl');
  log('pixi-calc: gl ctx', gl ? 'ok' : 'null');
  if (!gl) throw new Error('no webgl context');

  const renderer = new PIXI.Renderer({
    view: canvas,
    context: gl,
    width: canvas.width,
    height: canvas.height,
    antialias: false,
    backgroundColor: 0x111111,
  });
  log(
    'pixi-calc: renderer',
    renderer && renderer.type !== undefined ? String(renderer.type) : 'unknown',
  );

  const stage = new PIXI.Container();
  const program = mkProgram(PIXI);

  // Static calculator-like scene (first-pass visual only).
  stage.addChild(mkRectMesh(PIXI, program, 40, 28, 560, 360, [34, 34, 34]));
  stage.addChild(mkRectMesh(PIXI, program, 60, 78, 520, 42, [27, 27, 27]));
  stage.addChild(mkRectMesh(PIXI, program, 60, 128, 520, 42, [27, 27, 27]));
  stage.addChild(mkRectMesh(PIXI, program, 60, 218, 520, 70, [24, 24, 24]));

  const btnY = 310;
  const btnW = 78;
  const btnH = 50;
  const gap = 12;
  const startX = 60;
  stage.addChild(mkRectMesh(PIXI, program, startX + 0 * (btnW + gap), btnY, btnW, btnH, [42, 42, 42]));
  stage.addChild(mkRectMesh(PIXI, program, startX + 1 * (btnW + gap), btnY, btnW, btnH, [42, 42, 42]));
  stage.addChild(mkRectMesh(PIXI, program, startX + 2 * (btnW + gap), btnY, btnW, btnH, [42, 42, 42]));
  stage.addChild(mkRectMesh(PIXI, program, startX + 3 * (btnW + gap), btnY, btnW, btnH, [42, 42, 42]));
  stage.addChild(mkRectMesh(PIXI, program, startX + 4 * (btnW + gap), btnY, 118, btnH, [42, 42, 42]));

  // Tiny animated marker to prove the render loop is alive.
  const marker0 = [550, 55, 585, 55, 567, 80];
  const marker = mkTriangleMesh(PIXI, program, marker0, [255, 110, 80]);
  stage.addChild(marker.mesh);
  const markerPos = marker.geometry.getBuffer('aVertexPosition').data;
  const base = new Float32Array(markerPos);
  const cx = 567;
  const cy = 67;
  let angle = 0;

  while (true) {
    angle += 0.03;
    const c = Math.cos(angle);
    const s = Math.sin(angle);
    for (let i = 0; i < 3; i++) {
      const x0 = base[i * 2] - cx;
      const y0 = base[i * 2 + 1] - cy;
      markerPos[i * 2] = cx + (x0 * c - y0 * s);
      markerPos[i * 2 + 1] = cy + (x0 * s + y0 * c);
    }
    marker.geometry.getBuffer('aVertexPosition').update();
    renderer.render(stage);
  }
} catch (e) {
  log('pixi-calc: error', (e && e.stack) ? e.stack : (e && e.message) ? e.message : String(e));
}

export {};
