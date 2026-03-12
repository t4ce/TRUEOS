import * as cmdStream from 'trueos:cmd_stream';
import { DEFAULT_THEME } from './theme.mjs';

const ATLAS_KIND = 1;
const LI_ICON_ID = 5;
const SUMMARY_ICON_ID = 0;
const RADIO_ICON_ID = LI_ICON_ID + 1;
const CHECKBOX_ICON_ID = LI_ICON_ID + 2;
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
const HR_RGBA = 0x000000ff;
const HR_TOP_PAD = DEFAULT_THEME.TOP_PAD;
const HR_BOTTOM_PAD = DEFAULT_THEME.TOP_PAD;
const ITALIC_SLANT_DEG = 12;

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function iconIdForRowKind(kind) {
  if (kind === 'summary-text') return SUMMARY_ICON_ID;
  if (kind === 'radio-text') return RADIO_ICON_ID;
  if (kind === 'checkbox-text') return CHECKBOX_ICON_ID;
  if (kind === 'link-text') return LINK_ICON_ID;
  if (kind === 'li-text') return LI_ICON_ID;
  return -1;
}

function rowItalicTiltDeg(row) {
  const fontStyle = String(row && row.style && row.style.fontStyle || '').toLowerCase();
  if (fontStyle === 'italic' || fontStyle === 'oblique') return ITALIC_SLANT_DEG;
  return 0;
}

function rowIsBold(row) {
  const raw = row && row.style && row.style.fontWeight;
  if (raw == null) return 0;
  const fontWeight = String(raw).toLowerCase();
  if (fontWeight === 'bold' || fontWeight === 'bolder') return 1;
  const numeric = Number(fontWeight);
  return Number.isFinite(numeric) && numeric >= 600 ? 1 : 0;
}

export function renderScene(doc, vw, vh, scrollY, overlayRuns, overlayRect = null) {
  const texId = Number(cmdStream.createAtlasTexture(ATLAS_KIND) || 0);
  const runs = [];
  const iconRuns = [];
  const imageRuns = [];
  const buttonRuns = [];
  const hrRuns = [];
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
      : (kind === 'hr'
        ? Math.max(1, Math.round(Number(row && row.heightPx || 0) || (1 + HR_TOP_PAD + HR_BOTTOM_PAD)))
        : DEFAULT_THEME.LINE_H);
    if (y < -boxH) continue;
    if (y > Number(vh || 0) + boxH) continue;
    if (kind === 'hr') {
      const width = Math.max(1, Math.round(Number(row && row.widthPx || 0) || (Number(vw || 0) - (DEFAULT_THEME.LEFT_PAD * 2))));
      const ruleHeight = Math.max(1, Math.round(Number(row && row.ruleHeightPx || 0) || 1));
      const ruleY = y + HR_TOP_PAD;
      hrRuns.push({ x, y: ruleY, width, height: ruleHeight });
      continue;
    }
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
    const iconId = iconIdForRowKind(kind);
    if (iconId >= 0) {
      const iconY = y
        + Math.round((DEFAULT_THEME.LINE_H - LI_ICON_SIZE) * 0.5)
        + LI_ICON_XY_NUDGE;
      iconRuns.push(
        iconId,
        x + LI_ICON_X_SHIFT + LI_ICON_XY_NUDGE,
        iconY,
        LI_ICON_PALETTE,
      );
      runs.push({
        x: x + LI_TEXT_X_OFFSET,
        y,
        text,
        rgba: DEFAULT_TEXT_RGBA,
        italicTiltDeg: rowItalicTiltDeg(row),
        boldMode: rowIsBold(row),
      });
      continue;
    }
    runs.push({
      x,
      y,
      text,
      rgba: DEFAULT_TEXT_RGBA,
      italicTiltDeg: rowItalicTiltDeg(row),
      boldMode: rowIsBold(row),
    });
  }
  if (Array.isArray(overlayRuns) && overlayRuns.length > 0) {
    for (let i = 0; i < overlayRuns.length; ) {
      const x = overlayRuns[i];
      const y = overlayRuns[i + 1];
      const text = overlayRuns[i + 2];
      const rgba = overlayRuns[i + 3];
      if (i + 2 >= overlayRuns.length) break;
      runs.push({
        x,
        y,
        text,
        rgba: Number.isFinite(Number(rgba)) ? Number(rgba) : DEFAULT_TEXT_RGBA,
        italicTiltDeg: 0,
        boldMode: 0,
      });
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
    for (let i = 0; i < hrRuns.length; i += 1) {
      const run = hrRuns[i];
      cmdStream.fillRect(run.x, run.y, run.width, run.height, HR_RGBA, 0, 0);
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
    for (let i = 0; i < runs.length; i += 1) {
      const run = runs[i] || null;
      const rgba = Number(run && run.rgba);
      cmdStream.drawAtlasText(
        texId,
        ATLAS_KIND,
        Number(run && run.x || 0),
        Number(run && run.y || 0),
        String(run && run.text || ''),
        DEFAULT_THEME.FONT_PX,
        Number.isFinite(rgba) ? ((rgba >>> 8) & 0x00FFFFFF) : DEFAULT_THEME.FONT_RGB,
        Number.isFinite(rgba) ? (rgba & 0xFF) : DEFAULT_THEME.FONT_ALPHA,
        Number(run && run.italicTiltDeg || 0),
        Number(run && run.boldMode || 0),
      );
    }
  } finally {
    cmdStream.endFrame();
  }

  return true;
}
