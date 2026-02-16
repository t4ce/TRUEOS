import * as PIXI from 'pixi.js@7.4.0?bundle&target=es2022';
import process from 'node:process';

globalThis.__trueos_webgl_force = 1;

const log = globalThis.print;
log('pixi-mandel: start');

function nowSeconds() {
  const t = process.hrtime();
  return t[0] + (t[1] / 1e9);
}

function mkProgram() {
  const vs = `precision mediump float;
attribute vec2 aVertexPosition;
attribute vec4 aColor;
uniform mat3 translationMatrix;
uniform mat3 projectionMatrix;
varying vec4 vColor;
void main() {
  vColor = aColor;
  vec3 pos = projectionMatrix * translationMatrix * vec3(aVertexPosition, 1.0);
  gl_Position = vec4(pos.xy, 0.0, 1.0);
}`;

  const fs = `precision mediump float;
varying vec4 vColor;
void main() {
  gl_FragColor = vColor;
}`;

  return PIXI.Program.from(vs, fs);
}

function mandelbrotColor(cx, cy) {
  const maxIter = 64;
  let zx = 0.0;
  let zy = 0.0;
  let i = 0;

  while (i < maxIter) {
    const x2 = zx * zx;
    const y2 = zy * zy;
    if (x2 + y2 > 4.0) break;
    const zxy = zx * zy;
    zx = x2 - y2 + cx;
    zy = 2.0 * zxy + cy;
    i++;
  }

  if (i === maxIter) {
    return [0, 0, 0];
  }

  const t = i / maxIter;
  const r = Math.min(255, Math.max(0, Math.floor(9.0 * (1.0 - t) * t * t * t * 255.0)));
  const g = Math.min(255, Math.max(0, Math.floor(15.0 * (1.0 - t) * (1.0 - t) * t * t * 255.0)));
  const b = Math.min(255, Math.max(0, Math.floor(8.5 * (1.0 - t) * (1.0 - t) * (1.0 - t) * t * 255.0)));
  return [r, g, b];
}

function viewportToComplex(px, py, w, h, centerX, centerY, scaleX) {
  const nx = px / w;
  const ny = py / h;
  const aspect = h / w;
  const scaleY = scaleX * aspect;
  const cx = centerX + (nx - 0.5) * scaleX;
  const cy = centerY + (ny - 0.5) * scaleY;
  return [cx, cy];
}

function buildMandelMesh(PIXI, program, width, height, cellsX, cellsY) {
  const cols = cellsX + 1;
  const rows = cellsY + 1;

  const positions = new Float32Array(cols * rows * 2);
  const colors = new Uint8Array(cols * rows * 4);
  const indices = new Uint16Array(cellsX * cellsY * 6);

  let vp = 0;
  let vc = 0;
  for (let y = 0; y <= cellsY; y++) {
    const py = (y * height) / cellsY;
    for (let x = 0; x <= cellsX; x++) {
      const px = (x * width) / cellsX;
      positions[vp++] = px;
      positions[vp++] = py;

      const c = mandelbrotColor(-2.2 + (px / width) * 3.0, -1.3 + (py / height) * 2.6);
      colors[vc++] = c[0];
      colors[vc++] = c[1];
      colors[vc++] = c[2];
      colors[vc++] = 255;
    }
  }

  let ip = 0;
  for (let y = 0; y < cellsY; y++) {
    for (let x = 0; x < cellsX; x++) {
      const i0 = y * cols + x;
      const i1 = i0 + 1;
      const i2 = i0 + cols;
      const i3 = i2 + 1;

      indices[ip++] = i0;
      indices[ip++] = i1;
      indices[ip++] = i2;

      indices[ip++] = i1;
      indices[ip++] = i3;
      indices[ip++] = i2;
    }
  }

  const geometry = new PIXI.Geometry()
    .addAttribute('aVertexPosition', positions, 2, false, PIXI.TYPES.FLOAT)
    .addAttribute('aColor', colors, 4, true, PIXI.TYPES.UNSIGNED_BYTE)
    .addIndex(indices);

  const shader = new PIXI.Shader(program, {});
  return { mesh: new PIXI.Mesh(geometry, shader), colors };
}

function recolorMandel(colors, width, height, cellsX, cellsY, centerX, centerY, scaleX) {
  let vc = 0;
  for (let y = 0; y <= cellsY; y++) {
    const py = (y * height) / cellsY;
    for (let x = 0; x <= cellsX; x++) {
      const px = (x * width) / cellsX;
      const p = viewportToComplex(px, py, width, height, centerX, centerY, scaleX);
      const c = mandelbrotColor(p[0], p[1]);
      colors[vc++] = c[0];
      colors[vc++] = c[1];
      colors[vc++] = c[2];
      colors[vc++] = 255;
    }
  }
}

try {
  const width = 640;
  const height = 420;

  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;

  const gl = canvas.getContext('webgl');
  log('pixi-mandel: gl ctx', gl ? 'ok' : 'null');
  if (!gl) throw new Error('no webgl context');

  const renderer = new PIXI.Renderer({
    view: canvas,
    context: gl,
    width,
    height,
    antialias: false,
    backgroundColor: 0x000000,
  });
  log('pixi-mandel: renderer', renderer && renderer.type !== undefined ? String(renderer.type) : 'unknown');

  const stage = new PIXI.Container();
  const program = mkProgram();
  const cellsX = 128;
  const cellsY = 84;
  const mandel = buildMandelMesh(PIXI, program, width, height, cellsX, cellsY);
  stage.addChild(mandel.mesh);

  const colorBuffer = mandel.mesh.geometry.getBuffer('aColor');
  const colors = colorBuffer.data;
  const targetX = -0.743643887037151;
  const targetY = 0.13182590420533;
  let scaleX = 3.0;
  let frames = 0;
  let lastFpsSec = nowSeconds();

  while (true) {
    recolorMandel(colors, width, height, cellsX, cellsY, targetX, targetY, scaleX);
    colorBuffer.update();
    scaleX = Math.max(0.00006, scaleX * 0.996);
    renderer.render(stage);
    frames++;

    const now = nowSeconds();
    const dt = now - lastFpsSec;
    if (dt >= 0.1) {
      const fps = Math.floor(frames / dt);
      log('pixi-mandel: fps', String(fps), 'scale', String(scaleX));
      frames = 0;
      lastFpsSec = now;
    }
  }
} catch (e) {
  log('pixi-mandel: error', (e && e.stack) ? e.stack : (e && e.message) ? e.message : String(e));
}

export {};
