import * as cmdStream from 'trueos:cmd_stream';
import { DEFAULT_THEME } from './theme.mjs';

const ATLAS_KIND = 1;
const LI_ICON_ID = 5;
const LI_ICON_PALETTE = 0;
const LI_ICON_X_SHIFT = 2;
const LI_ICON_SIZE = 16;
const LI_ICON_XY_NUDGE = -2;
const LI_TEXT_X_OFFSET = 14;

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

export function renderScene(doc, vw, vh, scrollY, overlayRuns, overlayRect = null) {
  const texId = Number(cmdStream.createAtlasTexture(ATLAS_KIND) || 0);
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
  cmdStream.beginFrame();
  try {
    if (overlayRect && typeof overlayRect === 'object') {
      cmdStream.fillRect(
        Number(overlayRect.x || 0),
        Number(overlayRect.y || 0),
        Number(overlayRect.width || 0),
        Number(overlayRect.height || 0),
        Number(overlayRect.rgba || 0),
      );
    }
    // Icon quads are textured RGBA, so keep standard alpha blending enabled.
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
