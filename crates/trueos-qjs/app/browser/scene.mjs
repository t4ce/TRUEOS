import * as cmdStream from 'trueos:cmd_stream';
import { DEFAULT_THEME } from './theme.mjs';

const ATLAS_KIND = 1;
const LI_ICON_ID = 5;
const LI_ICON_PALETTE = 0;
const LI_ICON_X_SHIFT = 2;
const LI_ICON_SIZE = 16;
const LI_ICON_XY_NUDGE = -2;
const LI_TEXT_X_OFFSET = 14;

const STRESS_ICON_TEST = false;
const STRESS_ICON_COUNT = 1000;
const STRESS_ICON_SHAPES = 12;
const STRESS_ICON_PALETTES = 5;
const STRESS_SPEED_MIN = 48;
const STRESS_SPEED_MAX = 138;
const STRESS_BOUNCE = 0.84;
const STRESS_GRAVITY_PX_S2 = 520;
const STRESS_FIXED_STEP_MS = 200;

let atlasTexId = 0;
let stressW = 0;
let stressH = 0;
let stressPos = null;
let stressVel = null;
let stressShape = null;
let stressPalette = null;
let stressLastTickMs = 0;

function ensureAtlasTexture() {
  if (atlasTexId > 0) return atlasTexId;
  const created = Number(cmdStream.createAtlasTexture(ATLAS_KIND) || 0);
  atlasTexId = Math.max(0, created | 0);
  return atlasTexId;
}

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function rebuildStressRuns(vw, vh) {
  const w = Math.max(1, Number(vw || 1) | 0);
  const h = Math.max(1, Number(vh || 1) | 0);
  if (stressPos && stressVel && stressW === w && stressH === h) return;

  stressW = w;
  stressH = h;
  stressPos = new Float32Array(STRESS_ICON_COUNT * 2);
  stressVel = new Float32Array(STRESS_ICON_COUNT * 2);
  stressShape = new Uint16Array(STRESS_ICON_COUNT);
  stressPalette = new Uint16Array(STRESS_ICON_COUNT);
  stressLastTickMs = Date.now();

  let seed = (((w * 73856093) ^ (h * 19349663) ^ 0x9E3779B9) >>> 0);
  const nextRand = () => {
    seed = ((seed * 1664525) + 1013904223) >>> 0;
    return seed / 4294967296;
  };

  const maxX = Math.max(1, w - LI_ICON_SIZE);
  const maxY = Math.max(1, h - LI_ICON_SIZE);
  for (let i = 0; i < STRESS_ICON_COUNT; i++) {
    const iconId = Math.floor(nextRand() * STRESS_ICON_SHAPES);
    const x = Math.floor(nextRand() * maxX);
    const y = Math.floor(nextRand() * maxY);
    const palette = Math.floor(nextRand() * STRESS_ICON_PALETTES);
    const angle = nextRand() * Math.PI * 2;
    const speed = STRESS_SPEED_MIN + (nextRand() * (STRESS_SPEED_MAX - STRESS_SPEED_MIN));
    const b = i * 2;
    stressPos[b] = x;
    stressPos[b + 1] = y;
    stressVel[b] = Math.cos(angle) * speed;
    stressVel[b + 1] = Math.sin(angle) * speed;
    stressShape[i] = iconId;
    stressPalette[i] = palette;
  }
}

function stepStressParticles(vh, scrollY) {
  if (!stressPos || !stressVel) return;
  const now = Date.now();
  let dtMs = now - stressLastTickMs;
  stressLastTickMs = now;
  if (!(dtMs > 0)) dtMs = STRESS_FIXED_STEP_MS;
  dtMs = STRESS_FIXED_STEP_MS;
  const dtSec = dtMs / 1000;

  for (let i = 0; i < STRESS_ICON_COUNT; i++) {
    const b = i * 2;
    stressVel[b + 1] += STRESS_GRAVITY_PX_S2 * dtSec;
  }

  const collisions = Number(cmdStream.stepIconCollisions(
    stressPos,
    stressVel,
    dtMs,
    LI_ICON_SIZE,
    STRESS_BOUNCE,
  ) || 0);
  if (!Number.isFinite(collisions)) return;

  for (let i = 0; i < STRESS_ICON_COUNT; i++) {
    const b = i * 2;
    const sy = Number(stressPos[b + 1] || 0) - Number(scrollY || 0);
    if (sy < -LI_ICON_SIZE || sy > Number(vh || 0) + LI_ICON_SIZE) continue;
    cmdStream.drawLyonIconInFrame(
      Number(stressShape[i] || 0),
      Number(stressPos[b] || 0),
      sy,
      Number(stressPalette[i] || 0),
    );
  }
}

export function renderScene(doc, vw, vh, scrollY, overlayRuns) {
  const texId = ensureAtlasTexture();
  if (texId <= 0) return false;

  const runs = [];
  const iconRuns = [];
  const rows = Array.isArray(doc && doc.rows) ? doc.rows : [];
  const rowX = Array.isArray(doc && doc.rowX) ? doc.rowX : [];
  const rowY = Array.isArray(doc && doc.rowY) ? doc.rowY : [];

  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    const x = Math.round(Number(rowX[i] ?? DEFAULT_THEME.LEFT_PAD));
    const y = Math.round(Number(rowY[i] ?? (i * DEFAULT_THEME.LINE_H)) - Number(scrollY || 0));
    if (y < -DEFAULT_THEME.LINE_H) continue;
    if (y > Number(vh || 0) + DEFAULT_THEME.LINE_H) continue;
    const text = collapseWhitespace(String(row && row.text || ''));
    if (!text) continue;
    if (String(row && row.kind || '') === 'li-text') {
      const iconY = y
        + Math.round((DEFAULT_THEME.LINE_H - LI_ICON_SIZE) * 0.5)
        + LI_ICON_XY_NUDGE;
      iconRuns.push(x + LI_ICON_X_SHIFT + LI_ICON_XY_NUDGE, iconY, LI_ICON_PALETTE);
      runs.push(x + LI_TEXT_X_OFFSET, y, text);
      continue;
    }
    runs.push(x, y, text);
  }

  if (Array.isArray(overlayRuns) && overlayRuns.length > 0) {
    for (let i = 0; i < overlayRuns.length; i++) {
      runs.push(overlayRuns[i]);
    }
  }

  cmdStream.setClearRgb(DEFAULT_THEME.CLEAR_RGB);
  cmdStream.setViewport(Math.max(1, Number(vw || 1) | 0), Math.max(1, Number(vh || 1) | 0));
  if (STRESS_ICON_TEST) {
    rebuildStressRuns(vw, vh);
  }
  cmdStream.beginFrame();
  try {
    // Icon quads are textured RGBA now, so keep standard alpha blending enabled.
    cmdStream.setBlendEnabled(1);
    cmdStream.setBlendMode(0);
    cmdStream.setPremultipliedAlpha(0);
    for (let i = 0; i + 2 < iconRuns.length; i += 3) {
      cmdStream.drawLyonIconInFrame(
        LI_ICON_ID,
        Number(iconRuns[i] || 0),
        Number(iconRuns[i + 1] || 0),
        Number(iconRuns[i + 2] || 0),
      );
    }
    if (STRESS_ICON_TEST) {
      stepStressParticles(vh, scrollY);
    }

    // Text quads share the same alpha blend path.
    for (let i = 0; i + 2 < runs.length; i += 3) {
      cmdStream.drawAtlasText(
        texId,
        ATLAS_KIND,
        Number(runs[i] || 0),
        Number(runs[i + 1] || 0),
        String(runs[i + 2] || ''),
        DEFAULT_THEME.FONT_PX,
        DEFAULT_THEME.FONT_RGB,
        DEFAULT_THEME.FONT_ALPHA,
      );
    }
  } finally {
    cmdStream.endFrame();
  }

  return true;
}
