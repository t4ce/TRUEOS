const cmd = await import('cmd_stream');
const processMod = await import('node:process');
const process = processMod.default || processMod;
const VIEW_W = 1280;
const VIEW_H = 800;

const log = globalThis.print;
log('pixi-ui: start');

function nowSeconds() {
  const t = process.hrtime();
  return t[0] + (t[1] / 1e9);
}

function pushVtx(out, x, y, r, g, b) {
  // cmd_stream draw payload currently uses clip-space XY.
  const nx = (2.0 * (x / VIEW_W)) - 1.0;
  const ny = 1.0 - (2.0 * (y / VIEW_H));
  const dv = new DataView(new ArrayBuffer(12));
  dv.setFloat32(0, nx, true);
  dv.setFloat32(4, ny, true);
  dv.setUint8(8, r & 255);
  dv.setUint8(9, g & 255);
  dv.setUint8(10, b & 255);
  dv.setUint8(11, 0);
  out.push(
    dv.getUint8(0), dv.getUint8(1), dv.getUint8(2), dv.getUint8(3),
    dv.getUint8(4), dv.getUint8(5), dv.getUint8(6), dv.getUint8(7),
    dv.getUint8(8), dv.getUint8(9), dv.getUint8(10), dv.getUint8(11),
  );
}

function emitRect(out, x, y, w, h, r, g, b) {
  const x0 = x, y0 = y;
  const x1 = x + w, y1 = y;
  const x2 = x + w, y2 = y + h;
  const x3 = x, y3 = y + h;
  pushVtx(out, x0, y0, r, g, b);
  pushVtx(out, x1, y1, r, g, b);
  pushVtx(out, x2, y2, r, g, b);
  pushVtx(out, x0, y0, r, g, b);
  pushVtx(out, x2, y2, r, g, b);
  pushVtx(out, x3, y3, r, g, b);
}

function emitTri(out, x0, y0, x1, y1, x2, y2, r, g, b) {
  pushVtx(out, x0, y0, r, g, b);
  pushVtx(out, x1, y1, r, g, b);
  pushVtx(out, x2, y2, r, g, b);
}

function buildTriangleField(width, height, triCount) {
  const bytes = [];
  const cols = Math.max(1, Math.floor(Math.sqrt(triCount * (width / height))));
  const rows = Math.max(1, Math.ceil(triCount / cols));
  const cellW = width / cols;
  const cellH = height / rows;
  let emitted = 0;

  for (let y = 0; y < rows && emitted < triCount; y++) {
    for (let x = 0; x < cols && emitted < triCount; x++) {
      const xBase = x * cellW;
      const yBase = y * cellH;
      const padX = cellW * 0.08;
      const padY = cellH * 0.08;
      const x0 = xBase + padX;
      const y0 = yBase + cellH - padY;
      const x1 = xBase + cellW - padX;
      const y1 = yBase + cellH - padY;
      const x2 = xBase + cellW * 0.5;
      const y2 = yBase + padY;
      const r = 60 + ((x * 17 + y * 13) % 160);
      const g = 70 + ((x * 11 + y * 19) % 150);
      const b = 80 + ((x * 23 + y * 7) % 140);
      emitTri(bytes, x0, y0, x1, y1, x2, y2, r, g, b);
      emitted++;
    }
  }

  return new Uint8Array(bytes);
}

try {
  const width = VIEW_W;
  const height = VIEW_H;
  cmd.setViewport(width, height);
  cmd.setBlendEnabled(false);
  cmd.setClearRgb(0x12161d);
  log('pixi-ui: cmd-stream ok');
  const stages = [1000, 10000, 50000, 100000];
  const reportSec = 5.0;
  const stageSec = 15.0;
  let stageIdx = 0;
  let stageTriCount = stages[stageIdx];
  let stageBytes = buildTriangleField(width, height, stageTriCount);
  log('pixi-ui: bench stage tris', String(stageTriCount), 'bytes', String(stageBytes.length));

  let stageStartSec = nowSeconds();
  let reportStartSec = stageStartSec;
  let frames = 0;
  let frameSecAccum = 0.0;
  while (true) {
    const frameStartSec = nowSeconds();

    cmd.beginFrame();
    cmd.drawTrianglesU8(stageBytes);
    cmd.endFrame();

    const frameEndSec = nowSeconds();
    frameSecAccum += (frameEndSec - frameStartSec);
    frames++;

    const nowSec = frameEndSec;
    const reportDt = nowSec - reportStartSec;
    if (reportDt >= reportSec && frames > 0) {
      const fps = frames / reportDt;
      const msPerFrame = (frameSecAccum / frames) * 1000.0;
      const mtrisPerSec = (stageTriCount * fps) / 1000000.0;
      log(
        'pixi-ui: bench',
        'tris/frame', String(stageTriCount),
        'fps', String(Math.floor(fps)),
        'ms/frame', String(msPerFrame),
        'Mtris/s', String(mtrisPerSec),
      );
      reportStartSec = nowSec;
      frames = 0;
      frameSecAccum = 0.0;
    }

    const stageDt = nowSec - stageStartSec;
    if (stageDt >= stageSec) {
      stageIdx = (stageIdx + 1) % stages.length;
      stageTriCount = stages[stageIdx];
      stageBytes = buildTriangleField(width, height, stageTriCount);
      log('pixi-ui: bench stage tris', String(stageTriCount), 'bytes', String(stageBytes.length));
      stageStartSec = nowSec;
      reportStartSec = nowSec;
      frames = 0;
      frameSecAccum = 0.0;
    }
  }
} catch (e) {
  log('pixi-ui: error', (e && e.stack) ? e.stack : (e && e.message) ? e.message : String(e));
}

export {};
