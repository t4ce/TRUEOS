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
const DEFAULT_CLEAR_RGBA = ((DEFAULT_THEME.CLEAR_RGB & 0x00FFFFFF) << 8) | 0xFF;
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
const DEBUG_TEXT_BG_RGBA = 0xffef9cff;
const DEBUG_ICON_BG_RGBA = 0xffcc80ff;
const DEBUG_IMAGE_BG_RGBA = 0xc5e1a5ff;
const DEBUG_BUTTON_BG_RGBA = 0x90caf9ff;
const DEBUG_LINK_BG_RGBA = 0xa5d6a7ff;
const DEBUG_HR_BG_RGBA = 0xf48fb1ff;
const DEBUG_REGION_TOP_RGBA = 0xff3b30ff;
const DEBUG_REGION_LEFT_RGBA = 0x00c7beff;

function browserTagRowsDebugEnabled() {
  const host = (typeof globalThis !== 'undefined') ? globalThis : this;
  return !!(host && host.__trueosBrowserShowClosingTagRows);
}

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

function estimateRunTextWidthPx(run) {
  const text = collapseWhitespace(String(run && run.text || ''));
  const fontPx = Number.isFinite(Number(run && run.fontPx))
    ? Math.max(1, Math.round(Number(run.fontPx)))
    : DEFAULT_THEME.FONT_PX;
  if (!text) return Math.max(4, Math.round(fontPx * 0.5));
  return Math.max(4, Math.round(text.length * fontPx * 0.56));
}

function estimateRunTextHeightPx(run) {
  const fontPx = Number.isFinite(Number(run && run.fontPx))
    ? Math.max(1, Math.round(Number(run.fontPx)))
    : DEFAULT_THEME.FONT_PX;
  return Math.max(DEFAULT_THEME.LINE_H, Math.ceil(fontPx * 1.35));
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

function buildOverlayTextRuns(overlayRuns) {
  const overlayTextRuns = [];
  if (!Array.isArray(overlayRuns) || overlayRuns.length <= 0) return overlayTextRuns;
  for (let i = 0; i < overlayRuns.length; ) {
    const x = overlayRuns[i];
    const y = overlayRuns[i + 1];
    const text = overlayRuns[i + 2];
    const rgba = overlayRuns[i + 3];
    if (i + 2 >= overlayRuns.length) break;
    overlayTextRuns.push({
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
  return overlayTextRuns;
}

function buildSceneDisplayLists(doc, vw, minY, maxY, overlayRuns, localOffsetY = 0) {
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
  const clipTop = Math.max(0, Number(minY || 0));
  const clipBottom = Math.max(clipTop, Number(maxY || 0));

  for (let i = 0; i < buttons.length; i += 1) {
    const button = buttons[i];
    const x = Math.round(Number(button && button.x || 0));
    const y = Math.round(Number(button && button.y || 0));
    const width = Math.max(1, Math.round(Number(button && button.width || 0)));
    const height = Math.max(1, Math.round(Number(button && button.height || 0)));
    if (y + height < clipTop) continue;
    if (y > clipBottom) continue;
    buttonRuns.push({ x, y, width, height });
  }

  for (let i = 0; i < interactives.length; i += 1) {
    const interactive = interactives[i];
    if (String(interactive && interactive.kind || '') !== 'link') continue;
    const x = Math.round(Number(interactive && interactive.x || 0));
    const y = Math.round(Number(interactive && interactive.y || 0));
    const width = Math.max(1, Math.round(Number(interactive && interactive.width || 0)));
    const height = Math.max(1, Math.round(Number(interactive && interactive.height || 0)));
    if (y + height < clipTop) continue;
    if (y > clipBottom) continue;
    linkRuns.push({ x, y, width, height });
  }

  for (let i = 0; i < rows.length; i += 1) {
    const row = rows[i];
    const x = Math.round(Number(rowX[i] ?? DEFAULT_THEME.LEFT_PAD));
    const y = Math.round(Number(rowY[i] ?? (i * DEFAULT_THEME.LINE_H)) + Number(localOffsetY || 0));
    const kind = String(row && row.kind || '');
    const boxH = kind === 'image'
      ? Math.max(1, Math.round(Number(row && row.heightPx || 0) || DEFAULT_THEME.LINE_H))
      : (kind === 'hr'
        ? Math.max(1, Math.round(Number(row && row.heightPx || 0) || (1 + HR_TOP_PAD + HR_BOTTOM_PAD)))
        : DEFAULT_THEME.LINE_H);
    if (y + boxH < clipTop) continue;
    if (y > clipBottom) continue;
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
      const iconY = y + Math.round((DEFAULT_THEME.LINE_H - LI_ICON_SIZE) * 0.5) + LI_ICON_XY_NUDGE;
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

  return {
    texId,
    runs,
    iconRuns,
    imageRuns,
    buttonRuns,
    linkRuns,
    hrRuns,
    overlayTextRuns: buildOverlayTextRuns(overlayRuns),
  };
}

function drawSceneDisplayLists(display, clipW, clipH, originY = 0, includeOverlayText = true, overlayRect = null) {
  const texId = Number(display && display.texId || 0);
  const runs = Array.isArray(display && display.runs) ? display.runs : [];
  const iconRuns = Array.isArray(display && display.iconRuns) ? display.iconRuns : [];
  const imageRuns = Array.isArray(display && display.imageRuns) ? display.imageRuns : [];
  const buttonRuns = Array.isArray(display && display.buttonRuns) ? display.buttonRuns : [];
  const linkRuns = Array.isArray(display && display.linkRuns) ? display.linkRuns : [];
  const hrRuns = Array.isArray(display && display.hrRuns) ? display.hrRuns : [];
  const overlayTextRuns = Array.isArray(display && display.overlayTextRuns) ? display.overlayTextRuns : [];

  if (overlayRect && typeof overlayRect === 'object') {
    cmdStream.fillRect(
      Number(overlayRect.x || 0),
      Number(overlayRect.y || 0),
      Number(overlayRect.width || 0),
      Number(overlayRect.height || 0),
      Number(overlayRect.rgba || 0),
    );
  }

  cmdStream.setOrigin(0, 0);
  cmdStream.pushClipRect(0, 0, clipW, clipH);
  cmdStream.setOrigin(0, originY);
  try {
    if (browserTagRowsDebugEnabled()) {
      for (let i = 0; i < buttonRuns.length; i += 1) {
        const run = buttonRuns[i];
        cmdStream.fillRect(run.x, run.y, run.width, run.height, DEBUG_BUTTON_BG_RGBA, 0, 0);
      }
      for (let i = 0; i < linkRuns.length; i += 1) {
        const run = linkRuns[i];
        cmdStream.fillRect(run.x, run.y, run.width, run.height, DEBUG_LINK_BG_RGBA, 0, 0);
      }
      for (let i = 0; i < hrRuns.length; i += 1) {
        const run = hrRuns[i];
        cmdStream.fillRect(run.x, run.y, run.width, run.height, DEBUG_HR_BG_RGBA, 0, 0);
      }
      for (let i = 0; i < imageRuns.length; i += 1) {
        const run = imageRuns[i];
        cmdStream.fillRect(run.x, run.y, run.width, run.height, DEBUG_IMAGE_BG_RGBA, 0, 0);
      }
      for (let i = 0; i < iconRuns.length; i += 1) {
        const run = iconRuns[i] || null;
        const x = Number(run && run.x || 0);
        const y = Number(run && run.y || 0);
        cmdStream.fillRect(x, y, LI_ICON_SIZE, LI_ICON_SIZE, DEBUG_ICON_BG_RGBA, 0, 0);
      }
      for (let i = 0; i < runs.length; i += 1) {
        const run = runs[i] || null;
        const x = Number(run && run.x || 0);
        const y = Number(run && run.y || 0);
        const width = estimateRunTextWidthPx(run);
        const height = estimateRunTextHeightPx(run);
        cmdStream.fillRect(x, y, width, height, DEBUG_TEXT_BG_RGBA, 0, 0);
      }
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
        cmdStream.drawTextureRect(
          run.texId,
          placement.drawX,
          placement.drawY,
          placement.drawWidth,
          placement.drawHeight,
          placement.u0,
          placement.v0,
          placement.u1,
          placement.v1,
        );
      }
      cmdStream.fillRect(run.x, run.y, run.width, run.height, IMAGE_STROKE_RGBA, 1, 0);
    }
    for (let i = 0; i < iconRuns.length; i += 1) {
      const run = iconRuns[i] || null;
      cmdStream.drawLyonIconInFrame(
        Number(run && run.iconId || 0),
        Number(run && run.x || 0),
        Number(run && run.y || 0),
        Number(run && run.colorId || 0),
      );
    }
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
    cmdStream.setOrigin(0, 0);
    cmdStream.popClipRect();
  }

  if (!includeOverlayText) return;

  for (let i = 0; i < overlayTextRuns.length; i += 1) {
    const run = overlayTextRuns[i] || null;
    const rgba = Number(run && run.rgba);
    cmdStream.drawAtlasText(
      texId,
      ATLAS_KIND,
      Number(run && run.x || 0),
      Number(run && run.y || 0),
      String(run && run.text || ''),
      Number.isFinite(Number(run && run.fontPx)) ? Math.max(1, Math.round(Number(run.fontPx))) : DEFAULT_THEME.FONT_PX,
      Number.isFinite(rgba) ? ((rgba >>> 8) & 0x00FFFFFF) : DEFAULT_THEME.FONT_RGB,
      Number.isFinite(rgba) ? (rgba & 0xFF) : DEFAULT_THEME.FONT_ALPHA,
      Number(run && run.italicTiltDeg || 0),
      Number(run && run.boldMode || 0),
    );
  }
}

export function renderSceneContentToCurrentTarget(doc, vw, contentH) {
  const targetW = Math.max(1, Number(vw || 1) | 0);
  const targetH = Math.max(1, Number(contentH || 1) | 0);
  const display = buildSceneDisplayLists(doc, targetW, 0, targetH, null);
  cmdStream.setOrigin(0, 0);
  cmdStream.fillRect(0, 0, targetW, targetH, DEFAULT_CLEAR_RGBA, 0, 0);
  drawSceneDisplayLists(display, targetW, targetH, 0, false, null);
  return true;
}

export function renderSceneRegionToCurrentTarget(doc, vw, docTopY = 0, regionH = 1) {
  const targetW = Math.max(1, Number(vw || 1) | 0);
  const regionTop = Math.max(0, Math.round(Number(docTopY || 0)));
  const targetH = Math.max(1, Number(regionH || 1) | 0);
  const display = buildSceneDisplayLists(doc, targetW, regionTop, regionTop + targetH, null, -regionTop);
  cmdStream.setViewport(targetW, targetH);
  cmdStream.setOrigin(0, 0);
  cmdStream.setBlendEnabled(0);
  cmdStream.setBlendMode(0);
  cmdStream.setPremultipliedAlpha(0);
  try {
    cmdStream.fillRect(0, 0, targetW, targetH, DEFAULT_CLEAR_RGBA, 0, 0);
    if (browserTagRowsDebugEnabled()) {
      cmdStream.fillRect(0, 0, targetW, Math.min(6, targetH), DEBUG_REGION_TOP_RGBA, 0, 0);
      cmdStream.fillRect(0, 0, Math.min(6, targetW), targetH, DEBUG_REGION_LEFT_RGBA, 0, 0);
    }
    drawSceneDisplayLists(display, targetW, targetH, 0, false, null);
  } finally {
    cmdStream.setBlendEnabled(0);
    cmdStream.setBlendMode(0);
    cmdStream.setPremultipliedAlpha(0);
  }
  return true;
}

export function composeSceneTextureToCurrentTarget(contentTexId, contentW, contentH, vw, vh, scrollY, contentTopY = 0, overlayRuns, overlayRect = null) {
  const targetTexId = Math.max(0, Number(contentTexId || 0) | 0);
  const drawW = Math.max(1, Number(vw || 1) | 0);
  const drawH = Math.max(1, Number(vh || 1) | 0);
  const texW = Math.max(1, Number(contentW || drawW) | 0);
  const texH = Math.max(1, Number(contentH || drawH) | 0);
  const initialTop = Math.max(0, Math.min(texH - 1, Math.round(Number(contentTopY || 0))));
  const maxScroll = Math.max(0, texH - initialTop - drawH);
  const scrollTop = initialTop + Math.max(0, Math.min(maxScroll, Math.round(Number(scrollY || 0))));
  const overlayTextRuns = buildOverlayTextRuns(overlayRuns);

  if (overlayRect && typeof overlayRect === 'object') {
    cmdStream.fillRect(
      Number(overlayRect.x || 0),
      Number(overlayRect.y || 0),
      Number(overlayRect.width || 0),
      Number(overlayRect.height || 0),
      Number(overlayRect.rgba || 0),
    );
  }

  cmdStream.setOrigin(0, 0);
  cmdStream.setBlendEnabled(0);
  cmdStream.setBlendMode(0);
  cmdStream.setPremultipliedAlpha(0);
  cmdStream.pushClipRect(0, 0, drawW, drawH);
  try {
    if (targetTexId > 0) {
      const u0 = 0;
      const v0 = Math.max(0, scrollTop / texH);
      const u1 = Math.min(1, drawW / texW);
      const v1 = Math.min(1, (scrollTop + drawH) / texH);
      cmdStream.drawTextureRect(targetTexId, 0, 0, drawW, drawH, u0, v0, u1, v1);
    }
  } finally {
    cmdStream.popClipRect();
  }

  if (overlayTextRuns.length <= 0) return true;

  cmdStream.setBlendEnabled(1);
  cmdStream.setBlendMode(0);
  cmdStream.setPremultipliedAlpha(0);
  const texId = Number(cmdStream.createAtlasTexture(ATLAS_KIND) || 0);
  for (let i = 0; i < overlayTextRuns.length; i += 1) {
    const run = overlayTextRuns[i] || null;
    const rgba = Number(run && run.rgba);
    cmdStream.drawAtlasText(
      texId,
      ATLAS_KIND,
      Number(run && run.x || 0),
      Number(run && run.y || 0),
      String(run && run.text || ''),
      Number.isFinite(Number(run && run.fontPx)) ? Math.max(1, Math.round(Number(run.fontPx))) : DEFAULT_THEME.FONT_PX,
      Number.isFinite(rgba) ? ((rgba >>> 8) & 0x00FFFFFF) : DEFAULT_THEME.FONT_RGB,
      Number.isFinite(rgba) ? (rgba & 0xFF) : DEFAULT_THEME.FONT_ALPHA,
      Number(run && run.italicTiltDeg || 0),
      Number(run && run.boldMode || 0),
    );
  }
  return true;
}

export function composeSceneRegionsToCurrentTarget(regions, vw, vh, scrollY, contentTopY = 0, overlayRuns, overlayRect = null) {
  const drawW = Math.max(1, Number(vw || 1) | 0);
  const drawH = Math.max(1, Number(vh || 1) | 0);
  const initialTop = Math.max(0, Math.round(Number(contentTopY || 0)));
  const scrollTop = initialTop + Math.max(0, Math.round(Number(scrollY || 0)));
  const scrollBottom = scrollTop + drawH;
  const overlayTextRuns = buildOverlayTextRuns(overlayRuns);
  const items = Array.isArray(regions) ? regions : [];

  cmdStream.setViewport(drawW, drawH);
  cmdStream.setOrigin(0, 0);
  if (overlayRect && typeof overlayRect === 'object') {
    cmdStream.fillRect(
      Number(overlayRect.x || 0),
      Number(overlayRect.y || 0),
      Number(overlayRect.width || 0),
      Number(overlayRect.height || 0),
      Number(overlayRect.rgba || 0),
    );
  }

  cmdStream.setBlendEnabled(0);
  cmdStream.setBlendMode(0);
  cmdStream.setPremultipliedAlpha(0);
  cmdStream.pushClipRect(0, 0, drawW, drawH);
  try {
    for (let i = 0; i < items.length; i += 1) {
      const region = items[i] || null;
      const texId = Math.max(0, Number(region && region.texId || 0) | 0);
      const texW = Math.max(1, Number(region && region.width || drawW) | 0);
      const texH = Math.max(1, Number(region && region.height || drawH) | 0);
      const docY = Math.max(0, Math.round(Number(region && region.docY || 0)));
      const docBottom = docY + texH;
      if (texId <= 0) continue;
      if (docBottom <= scrollTop || docY >= scrollBottom) continue;

      const srcTop = Math.max(docY, scrollTop);
      const srcBottom = Math.min(docBottom, scrollBottom);
      const srcHeight = Math.max(0, srcBottom - srcTop);
      if (srcHeight <= 0) continue;

      const srcOffsetY = srcTop - docY;
      const destY = srcTop - scrollTop;
      const drawWidth = Math.max(1, Math.min(drawW, texW));
      const u0 = 0;
      const u1 = Math.min(1, drawWidth / texW);
      const v0 = Math.max(0, srcOffsetY / texH);
      const v1 = Math.min(1, (srcOffsetY + srcHeight) / texH);
      cmdStream.drawTextureRect(texId, 0, destY, drawWidth, srcHeight, u0, v0, u1, v1);
    }
  } finally {
    cmdStream.popClipRect();
  }

  if (overlayTextRuns.length <= 0) return true;

  cmdStream.setBlendEnabled(1);
  cmdStream.setBlendMode(0);
  cmdStream.setPremultipliedAlpha(0);
  const texId = Number(cmdStream.createAtlasTexture(ATLAS_KIND) || 0);
  for (let i = 0; i < overlayTextRuns.length; i += 1) {
    const run = overlayTextRuns[i] || null;
    const rgba = Number(run && run.rgba);
    cmdStream.drawAtlasText(
      texId,
      ATLAS_KIND,
      Number(run && run.x || 0),
      Number(run && run.y || 0),
      String(run && run.text || ''),
      Number.isFinite(Number(run && run.fontPx)) ? Math.max(1, Math.round(Number(run.fontPx))) : DEFAULT_THEME.FONT_PX,
      Number.isFinite(rgba) ? ((rgba >>> 8) & 0x00FFFFFF) : DEFAULT_THEME.FONT_RGB,
      Number.isFinite(rgba) ? (rgba & 0xFF) : DEFAULT_THEME.FONT_ALPHA,
      Number(run && run.italicTiltDeg || 0),
      Number(run && run.boldMode || 0),
    );
  }
  cmdStream.setBlendEnabled(0);
  cmdStream.setBlendMode(0);
  cmdStream.setPremultipliedAlpha(0);
  return true;
}

export function renderScene(doc, vw, vh, scrollY, overlayRuns, overlayRect = null) {
  const targetW = Math.max(1, Number(vw || 1) | 0);
  const targetH = Math.max(1, Number(vh || 1) | 0);
  const scrollTop = Math.max(0, Number(scrollY || 0));
  const display = buildSceneDisplayLists(doc, targetW, scrollTop, scrollTop + targetH, overlayRuns);

  cmdStream.setClearRgb(DEFAULT_THEME.CLEAR_RGB);
  cmdStream.setViewport(targetW, targetH);
  cmdStream.beginFrame();
  try {
    drawSceneDisplayLists(display, targetW, targetH, -scrollTop, true, overlayRect);
  } finally {
    cmdStream.endFrame();
  }

  return true;
}
