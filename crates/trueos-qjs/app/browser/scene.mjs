import * as cmdStream from 'trueos:cmd_stream';
import { DEFAULT_THEME } from './theme.mjs';

const ATLAS_KIND = 1;

let atlasTexId = 0;

function ensureAtlasTexture() {
  if (atlasTexId > 0) return atlasTexId;
  const created = Number(cmdStream.createAtlasTexture(ATLAS_KIND) || 0);
  atlasTexId = Math.max(0, created | 0);
  return atlasTexId;
}

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

export function renderScene(doc, vw, vh, scrollY, overlayRuns) {
  const texId = ensureAtlasTexture();
  if (texId <= 0) return false;

  const runs = [];
  const rows = Array.isArray(doc && doc.rows) ? doc.rows : [];
  const rowX = Array.isArray(doc && doc.rowX) ? doc.rowX : [];
  const rowY = Array.isArray(doc && doc.rowY) ? doc.rowY : [];

  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    const x = Math.round(Number(rowX[i] || DEFAULT_THEME.LEFT_PAD));
    const y = Math.round(Number(rowY[i] || (i * DEFAULT_THEME.LINE_H)) - Number(scrollY || 0));
    if (y < -DEFAULT_THEME.LINE_H) continue;
    if (y > Number(vh || 0) + DEFAULT_THEME.LINE_H) continue;
    const text = collapseWhitespace(String(row && row.text || ''));
    if (!text) continue;
    runs.push(x, y, text);
  }

  if (Array.isArray(overlayRuns) && overlayRuns.length > 0) {
    for (let i = 0; i < overlayRuns.length; i++) {
      runs.push(overlayRuns[i]);
    }
  }

  cmdStream.setClearRgb(DEFAULT_THEME.CLEAR_RGB);
  cmdStream.setViewport(Math.max(1, Number(vw || 1) | 0), Math.max(1, Number(vh || 1) | 0));
  cmdStream.beginFrame();
  try {
    cmdStream.setBlendEnabled(1);
    cmdStream.setBlendMode(0);
    cmdStream.setPremultipliedAlpha(0);
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
