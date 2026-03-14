import * as cmdStream from 'trueos:cmd_stream';
import { DEFAULT_THEME } from './theme.mjs';

const ATLAS_KIND = 1;
const LYON_ICON = Object.freeze({
  ARROW_RIGHT: 0,
  ARROW_DOWN: 1,
  ARROW_LEFT: 2,
  ARROW_UP: 3,
  PLUS: 4,
  MINUS: 5,
  CIRCLE: 6,
  RECT: 7,
  TRIANGLE: 8,
  PENTAGON: 9,
  HEXAGON: 10,
  OCTAGON: 11,
});
const ROW_KIND_ICON = Object.freeze({
  'summary-text': LYON_ICON.ARROW_RIGHT,
  'radio-text': LYON_ICON.CIRCLE,
  'checkbox-text': LYON_ICON.RECT,
  'li-text': LYON_ICON.MINUS,
});
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
const LINK_OUTLINE_RGBA = 0x000000ff;
const HR_RGBA = 0x000000ff;
const HR_TOP_PAD = DEFAULT_THEME.TOP_PAD;
const HR_BOTTOM_PAD = DEFAULT_THEME.TOP_PAD;
const ITALIC_SLANT_DEG = 12;
const CHAMFER_PX = 5;
const LINK_UNDERLINE_THICKNESS_PX = 1;
const MAX_RENDER_TEXT_CHARS = 512;

function styleColorToTextRgba(style) {
  if (!style || typeof style !== 'object') return DEFAULT_TEXT_RGBA;
  const raw = String(style.color || '').trim().toLowerCase();
  if (!raw) return DEFAULT_TEXT_RGBA;
  if (raw === 'transparent') return 0x00000000;
  const hex = raw.match(/^#([0-9a-f]{6})$/i);
  if (!hex) return DEFAULT_TEXT_RGBA;
  const rgb = Number.parseInt(hex[1], 16);
  if (!Number.isFinite(rgb)) return DEFAULT_TEXT_RGBA;
  return ((rgb & 0x00FFFFFF) << 8) | (DEFAULT_THEME.FONT_ALPHA & 0xFF);
}

function rowFontPx(row) {
  const style = row && row.style && typeof row.style === 'object' ? row.style : null;
  const raw = Number(style && style.fontSizePx);
  if (!Number.isFinite(raw) || raw <= 0) return DEFAULT_THEME.FONT_PX;
  return Math.max(1, Math.round(raw));
}

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function iconIdForRowKind(kind) {
  const iconId = ROW_KIND_ICON[String(kind || '')];
  return Number.isInteger(iconId) ? iconId : -1;
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

function pxToNdcX(x, vw) {
  return ((Number(x || 0) / Math.max(1, Number(vw || 1))) * 2) - 1;
}

function pxToNdcY(y, vh) {
  return 1 - ((Number(y || 0) / Math.max(1, Number(vh || 1))) * 2);
}

function writeTexturedVertex(view, vertexIndex, x, y, u, v, r, g, b, a) {
  const base = vertexIndex * 20;
  view.setFloat32(base, x, true);
  view.setFloat32(base + 4, y, true);
  view.setFloat32(base + 8, u, true);
  view.setFloat32(base + 12, v, true);
  view.setUint8(base + 16, r);
  view.setUint8(base + 17, g);
  view.setUint8(base + 18, b);
  view.setUint8(base + 19, a);
}

function buildTexturedQuadVertices(x, y, width, height, vw, vh, u0 = 0, v0 = 0, u1 = 1, v1 = 1) {
  const x0 = pxToNdcX(x, vw);
  const y0 = pxToNdcY(y, vh);
  const x1 = pxToNdcX(x + width, vw);
  const y1 = pxToNdcY(y + height, vh);
  const buffer = new ArrayBuffer(6 * 20);
  const view = new DataView(buffer);

  writeTexturedVertex(view, 0, x0, y1, u0, v1, 255, 255, 255, 255);
  writeTexturedVertex(view, 1, x1, y1, u1, v1, 255, 255, 255, 255);
  writeTexturedVertex(view, 2, x1, y0, u1, v0, 255, 255, 255, 255);
  writeTexturedVertex(view, 3, x0, y1, u0, v1, 255, 255, 255, 255);
  writeTexturedVertex(view, 4, x1, y0, u1, v0, 255, 255, 255, 255);
  writeTexturedVertex(view, 5, x0, y0, u0, v0, 255, 255, 255, 255);

  return new Uint8Array(buffer);
}

function buildCenteredImagePlacement(run) {
  const boxWidth = Math.max(1, Math.round(Number(run && run.width || 0) || 1));
  const boxHeight = Math.max(1, Math.round(Number(run && run.height || 0) || 1));
  const pixelWidth = Math.max(0, Math.round(Number(run && run.pixelWidth || 0) || 0));
  const pixelHeight = Math.max(0, Math.round(Number(run && run.pixelHeight || 0) || 0));
  if (pixelWidth <= 0 || pixelHeight <= 0) {
    return {
      drawX: Number(run && run.x || 0),
      drawY: Number(run && run.y || 0),
      drawWidth: boxWidth,
      drawHeight: boxHeight,
      u0: 0,
      v0: 0,
      u1: 1,
      v1: 1,
    };
  }

  const drawWidth = Math.max(1, Math.min(boxWidth, pixelWidth));
  const drawHeight = Math.max(1, Math.min(boxHeight, pixelHeight));
  const drawX = Number(run && run.x || 0) + Math.floor((boxWidth - drawWidth) * 0.5);
  const drawY = Number(run && run.y || 0) + Math.floor((boxHeight - drawHeight) * 0.5);
  const cropLeft = Math.max(0, (pixelWidth - drawWidth) * 0.5);
  const cropTop = Math.max(0, (pixelHeight - drawHeight) * 0.5);
  const u0 = cropLeft / pixelWidth;
  const v0 = cropTop / pixelHeight;
  const u1 = (cropLeft + drawWidth) / pixelWidth;
  const v1 = (cropTop + drawHeight) / pixelHeight;
  return { drawX, drawY, drawWidth, drawHeight, u0, v0, u1, v1 };
}

export function renderScene(doc, vw, vh, scrollY, overlayRuns, overlayRect = null) {
  const texId = Number(cmdStream.createAtlasTexture(ATLAS_KIND) || 0);
  const runs = [];
  const iconRuns = [];
  const imageRuns = [];
  const buttonRuns = [];
  const linkRuns = [];
  const hrRuns = [];
  const rows = Array.isArray(doc && doc.rows) ? doc.rows : [];
  const rowX = Array.isArray(doc && doc.rowX) ? doc.rowX : [];
  const rowY = Array.isArray(doc && doc.rowY) ? doc.rowY : [];
  const themeLayout = doc && typeof doc === 'object' ? doc.themeLayout : null;
  const buttons = Array.isArray(themeLayout && themeLayout.buttons) ? themeLayout.buttons : [];
  const interactives = Array.isArray(themeLayout && themeLayout.interactives) ? themeLayout.interactives : [];

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

  for (let i = 0; i < interactives.length; i += 1) {
    const interactive = interactives[i];
    if (String(interactive && interactive.kind || '') !== 'link') continue;
    const x = Math.round(Number(interactive && interactive.x || 0));
    const y = Math.round(Number(interactive && interactive.y || 0) - Number(scrollY || 0));
    const width = Math.max(1, Math.round(Number(interactive && interactive.width || 0)));
    const height = Math.max(1, Math.round(Number(interactive && interactive.height || 0)));
    if (y < -height) continue;
    if (y > Number(vh || 0) + height) continue;
    linkRuns.push({ x, y, width, height });
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
      imageRuns.push({
        x: innerX,
        y: innerY,
        width: innerWidth,
        height: innerHeight,
        texId: Math.max(0, Number(row && row.texId || 0)),
        pixelWidth: Math.max(0, Number(row && row.imagePixelWidth || 0) | 0),
        pixelHeight: Math.max(0, Number(row && row.imagePixelHeight || 0) | 0),
      });
      continue;
    }
    const text = collapseWhitespace(String(row && row.text || ''));
    if (!text) continue;
    const iconId = iconIdForRowKind(kind);
    if (iconId >= 0) {
      const iconY = y
        + Math.round((DEFAULT_THEME.LINE_H - LI_ICON_SIZE) * 0.5)
        + LI_ICON_XY_NUDGE;
      iconRuns.push({
        iconId,
        colorId: LI_ICON_PALETTE,
        x: x + LI_ICON_X_SHIFT + LI_ICON_XY_NUDGE,
        y: iconY,
      });
      runs.push({
        x: x + LI_TEXT_X_OFFSET,
        y,
        text,
        rgba: styleColorToTextRgba(row && row.style),
        fontPx: rowFontPx(row),
        italicTiltDeg: rowItalicTiltDeg(row),
        boldMode: rowIsBold(row),
      });
      continue;
    }
    runs.push({
      x,
      y,
      text,
      rgba: styleColorToTextRgba(row && row.style),
      fontPx: rowFontPx(row),
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
        fontPx: DEFAULT_THEME.FONT_PX,
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
    for (let i = 0; i < linkRuns.length; i += 1) {
      const run = linkRuns[i];
      const left = Number(run.x || 0);
      const top = Number(run.y || 0);
      const width = Math.max(1, Number(run.width || 0));
      const height = Math.max(1, Number(run.height || 0));
      const right = left + width;
      const bottom = top + height - 1;
      const chamfer = Math.max(1, Math.min(CHAMFER_PX, Math.floor(width * 0.5)));
      const leftFlat = left + chamfer;
      const rightFlat = right - chamfer;
      const chamferTopY = bottom - chamfer;

      cmdStream.drawLine(left, chamferTopY, leftFlat, bottom, LINK_OUTLINE_RGBA, LINK_UNDERLINE_THICKNESS_PX);
      if (rightFlat > leftFlat) {
        cmdStream.drawLine(leftFlat, bottom, rightFlat, bottom, LINK_OUTLINE_RGBA, LINK_UNDERLINE_THICKNESS_PX);
      }
      cmdStream.drawLine(rightFlat, bottom, right, chamferTopY, LINK_OUTLINE_RGBA, LINK_UNDERLINE_THICKNESS_PX);
    }
    for (let i = 0; i < hrRuns.length; i += 1) {
      const run = hrRuns[i];
      cmdStream.fillRect(run.x, run.y, run.width, run.height, HR_RGBA, 0, 0);
    }
    for (let i = 0; i < imageRuns.length; i += 1) {
      const run = imageRuns[i];
      cmdStream.fillRect(run.x, run.y, run.width, run.height, IMAGE_FILL_RGBA, 0, 0);
    }
    cmdStream.setBlendEnabled(1);
    cmdStream.setBlendMode(0);
    cmdStream.setPremultipliedAlpha(0);
    for (let i = 0; i < imageRuns.length; i += 1) {
      const run = imageRuns[i];
      if (run.texId > 0) {
        const placement = buildCenteredImagePlacement(run);
        cmdStream.drawTexturedTrianglesU8(
          run.texId,
          buildTexturedQuadVertices(
            placement.drawX,
            placement.drawY,
            placement.drawWidth,
            placement.drawHeight,
            vw,
            vh,
            placement.u0,
            placement.v0,
            placement.u1,
            placement.v1,
          ),
        );
      }
      cmdStream.fillRect(run.x, run.y, run.width, run.height, IMAGE_STROKE_RGBA, 1, 0);
    }
    // Icon quads are textured RGBA, so keep standard alpha blending enabled.
    for (let i = 0; i < iconRuns.length; i += 1) {
      const run = iconRuns[i] || null;
      cmdStream.drawLyonIconInFrame(
        Number(run && run.iconId || 0),
        Number(run && run.x || 0),
        Number(run && run.y || 0),
        Number(run && run.colorId || 0),
      );
    }
    // Text quads share the same alpha blend path.
    for (let i = 0; i < runs.length; i += 1) {
      const run = runs[i] || null;
      const rgba = Number(run && run.rgba);
      const rawText = String(run && run.text || '');
      const renderText = rawText.length > MAX_RENDER_TEXT_CHARS
        ? rawText.slice(0, MAX_RENDER_TEXT_CHARS)
        : rawText;
      if (renderText.length !== rawText.length) {
        try {
          console.log(`[browser.scene] drawAtlasText truncated from ${rawText.length} to ${MAX_RENDER_TEXT_CHARS} chars`);
        } catch (_) {}
      }
      cmdStream.drawAtlasText(
        texId,
        ATLAS_KIND,
        Number(run && run.x || 0),
        Number(run && run.y || 0),
        renderText,
        Number.isFinite(Number(run && run.fontPx)) ? Math.max(1, Math.round(Number(run.fontPx))) : DEFAULT_THEME.FONT_PX,
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
