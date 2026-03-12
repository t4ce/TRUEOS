import * as cmdStream from 'trueos:cmd_stream';
import { DEFAULT_THEME } from './theme.mjs';

const ATLAS_KIND = 1;
const LI_ICON_ID = 5;
const LINK_ICON_ID = LI_ICON_ID + 1;
const LI_ICON_PALETTE = 0;
const LI_ICON_X_SHIFT = 2;
const LI_ICON_SIZE = 16;
const LI_ICON_XY_NUDGE = -2;
export const LI_TEXT_X_OFFSET = 14;
const IMAGE_FILL_RGBA = 0xebe6d6ff;
const IMAGE_STROKE_RGBA = 0x2f2a22ff;
const DEFAULT_TEXT_RGBA = ((DEFAULT_THEME.FONT_RGB & 0x00FFFFFF) << 8) | (DEFAULT_THEME.FONT_ALPHA & 0xFF);
const IMAGE_LEFT_PAD = DEFAULT_THEME.LEFT_PAD;
const IMAGE_TOP_PAD = DEFAULT_THEME.TOP_PAD;
const IMAGE_BOTTOM_PAD = DEFAULT_THEME.TOP_PAD;
const BUTTON_OUTLINE_RGBA = 0x000000ff;

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

export function renderScene(doc, vw, vh, scrollY, overlayRuns, overlayRect = null) {
  const texId = Number(cmdStream.createAtlasTexture(ATLAS_KIND) || 0);
  const runs = [];
  const iconRuns = [];
  const imageRuns = [];
  const buttonRuns = [];
  const rows = Array.isArray(doc && doc.rows) ? doc.rows : [];
  const rowX = Array.isArray(doc && doc.rowX) ? doc.rowX : [];
  const rowY = Array.isArray(doc && doc.rowY) ? doc.rowY : [];
  const themeLayout = doc && typeof doc === 'object' ? doc.themeLayout : null;
  const buttons = Array.isArray(themeLayout && themeLayout.buttons) ? themeLayout.buttons : [];

  for (let i = 0; i < buttons.length; i += 1) {
    const button = buttons[i];
    const x = Math.round(Number(button && button.x || 0));
    const y = Math.round(Number(button && button.y || 0) - Number(scrollY || 0));
    const width = Math.max(1, Math.round(Number(button && button.width || 0)));
    const height = Math.max(1, Math.round(Number(button && button.height || 0)));
    if (y < -height) continue;
    if (y > Number(vh || 0) + height) continue;
    buttonRuns.push({ x, y, width, height });
  }

  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    const x = Math.round(Number(rowX[i] ?? DEFAULT_THEME.LEFT_PAD));
    const y = Math.round(Number(rowY[i] ?? (i * DEFAULT_THEME.LINE_H)) - Number(scrollY || 0));
    const kind = String(row && row.kind || '');
    const boxH = kind === 'image'
      ? Math.max(1, Math.round(Number(row && row.heightPx || 0) || DEFAULT_THEME.LINE_H))
      : DEFAULT_THEME.LINE_H;
    if (y < -boxH) continue;
    if (y > Number(vh || 0) + boxH) continue;
    if (kind === 'image') {
      const width = Math.max(1, Math.round(Number(row && row.widthPx || 0) || DEFAULT_THEME.LINE_H));
      const height = boxH;
      const innerX = x + IMAGE_LEFT_PAD;
      const innerY = y + IMAGE_TOP_PAD;
      const innerWidth = Math.max(1, width - IMAGE_LEFT_PAD);
      const innerHeight = Math.max(1, height - IMAGE_TOP_PAD - IMAGE_BOTTOM_PAD);
      imageRuns.push({ x: innerX, y: innerY, width: innerWidth, height: innerHeight });
      continue;
    }
    const text = collapseWhitespace(String(row && row.text || ''));
    if (!text) continue;
    if (kind === 'li-text' || kind === 'link-text') {
      const iconY = y
        + Math.round((DEFAULT_THEME.LINE_H - LI_ICON_SIZE) * 0.5)
        + LI_ICON_XY_NUDGE;
      iconRuns.push(
        kind === 'link-text' ? LINK_ICON_ID : LI_ICON_ID,
        x + LI_ICON_X_SHIFT + LI_ICON_XY_NUDGE,
        iconY,
        LI_ICON_PALETTE,
      );
      runs.push(x + LI_TEXT_X_OFFSET, y, text, DEFAULT_TEXT_RGBA);
      continue;
    }
    runs.push(x, y, text, DEFAULT_TEXT_RGBA);
  }
  if (Array.isArray(overlayRuns) && overlayRuns.length > 0) {
    for (let i = 0; i < overlayRuns.length; ) {
      const x = overlayRuns[i];
      const y = overlayRuns[i + 1];
      const text = overlayRuns[i + 2];
      const rgba = overlayRuns[i + 3];
      if (i + 2 >= overlayRuns.length) break;
      runs.push(x, y, text, Number.isFinite(Number(rgba)) ? Number(rgba) : DEFAULT_TEXT_RGBA);
      i += Number.isFinite(Number(rgba)) ? 4 : 3;
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
    for (let i = 0; i < buttonRuns.length; i += 1) {
      const run = buttonRuns[i];
      cmdStream.fillRect(run.x, run.y, run.width, run.height, BUTTON_OUTLINE_RGBA, 1, 1);
    }
    for (let i = 0; i < imageRuns.length; i += 1) {
      const run = imageRuns[i];
      cmdStream.fillRect(run.x, run.y, run.width, run.height, IMAGE_STROKE_RGBA, 1, 0);
    }
    // Icon quads are textured RGBA, so keep standard alpha blending enabled.
    cmdStream.setBlendEnabled(1);
    cmdStream.setBlendMode(0);
    cmdStream.setPremultipliedAlpha(0);
    for (let i = 0; i + 3 < iconRuns.length; i += 4) {
      cmdStream.drawLyonIconInFrame(
        Number(iconRuns[i] || 0),
        Number(iconRuns[i + 1] || 0),
        Number(iconRuns[i + 2] || 0),
        Number(iconRuns[i + 3] || 0),
      );
    }
    // Text quads share the same alpha blend path.
    for (let i = 0; i + 3 < runs.length; i += 4) {
      cmdStream.drawAtlasText(
        texId,
        ATLAS_KIND,
        Number(runs[i] || 0),
        Number(runs[i + 1] || 0),
        String(runs[i + 2] || ''),
        DEFAULT_THEME.FONT_PX,
        Number.isFinite(Number(runs[i + 3])) ? ((Number(runs[i + 3]) >>> 8) & 0x00FFFFFF) : DEFAULT_THEME.FONT_RGB,
        Number.isFinite(Number(runs[i + 3])) ? (Number(runs[i + 3]) & 0xFF) : DEFAULT_THEME.FONT_ALPHA,
      );
    }
  } finally {
    cmdStream.endFrame();
  }

  return true;
}
