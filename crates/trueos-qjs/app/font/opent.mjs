import * as opentypeNs from "../vendor/opentype.mjs";

const opentype = opentypeNs.default ?? opentypeNs;
const INJECTED_FONT_BYTES_KEY = "__trueosOpentDemoFontBytes";
const DEMO_TEXT = "RTO";
const DEMO_FONT_SIZE = 64;
const DEMO_PADDING = 10;

function fail(message) {
  throw new Error(`opent.mjs: ${message}`);
}

function injectedFontBytes() {
  const injected = globalThis[INJECTED_FONT_BYTES_KEY];
  if (injected instanceof ArrayBuffer) {
    return injected.slice(0);
  }
  if (injected instanceof Uint8Array) {
    return injected.buffer.slice(
      injected.byteOffset,
      injected.byteOffset + injected.byteLength,
    );
  }
  fail(`missing injected font bytes on globalThis.${INJECTED_FONT_BYTES_KEY}`);
}

function makeRgba(width, height) {
  const rgba = new Uint8Array(width * height * 4);
  for (let i = 0; i < rgba.length; i += 4) {
    rgba[i] = 255;
    rgba[i + 1] = 255;
    rgba[i + 2] = 255;
    rgba[i + 3] = 255;
  }
  return rgba;
}

function fillRect(rgba, width, height, x, y, w, h, color) {
  const x0 = Math.max(0, Math.floor(x));
  const y0 = Math.max(0, Math.floor(y));
  const x1 = Math.min(width, Math.ceil(x + w));
  const y1 = Math.min(height, Math.ceil(y + h));
  for (let py = y0; py < y1; py += 1) {
    for (let px = x0; px < x1; px += 1) {
      const base = (py * width + px) * 4;
      rgba[base] = color[0];
      rgba[base + 1] = color[1];
      rgba[base + 2] = color[2];
      rgba[base + 3] = color[3];
    }
  }
}

function strokeRect(rgba, width, height, x, y, w, h, thickness, color) {
  fillRect(rgba, width, height, x, y, w, thickness, color);
  fillRect(rgba, width, height, x, y + h - thickness, w, thickness, color);
  fillRect(rgba, width, height, x, y, thickness, h, color);
  fillRect(rgba, width, height, x + w - thickness, y, thickness, h, color);
}

function metricOrZero(value) {
  return Number.isFinite(value) ? value : 0;
}

function glyphSummary(font, ch) {
  const glyph = font.charToGlyph(ch);
  const metrics = glyph && typeof glyph.getMetrics === "function"
    ? glyph.getMetrics()
    : {};
  const advance = metricOrZero(glyph?.advanceWidth);
  const left = metricOrZero(metrics.xMin);
  const right = metricOrZero(metrics.xMax);
  const top = metricOrZero(metrics.yMax);
  const bottom = metricOrZero(metrics.yMin);
  const boxW = Math.max(1, right - left);
  const boxH = Math.max(1, top - bottom);
  return {
    ch,
    glyphIndex: metricOrZero(glyph?.index),
    advance,
    left,
    right,
    top,
    bottom,
    boxW,
    boxH,
  };
}

function buildMetricImage(font, text) {
  const scale = DEMO_FONT_SIZE / Math.max(1, font.unitsPerEm || 1);
  const summaries = Array.from(text, (ch) => glyphSummary(font, ch));
  let width = DEMO_PADDING;
  for (const glyph of summaries) {
    width += Math.max(12, Math.ceil(glyph.advance * scale)) + DEMO_PADDING;
  }
  const height = Math.max(
    48,
    Math.ceil(DEMO_PADDING * 2 + (font.ascender - font.descender) * scale),
  );
  const baselineY = Math.ceil(DEMO_PADDING + font.ascender * scale);
  const rgba = makeRgba(width, height);
  let penX = DEMO_PADDING;

  for (const glyph of summaries) {
    const advancePx = Math.max(12, Math.ceil(glyph.advance * scale));
    const boxX = penX + glyph.left * scale;
    const boxY = baselineY - glyph.top * scale;
    const boxW = Math.max(3, Math.ceil(glyph.boxW * scale));
    const boxH = Math.max(3, Math.ceil(glyph.boxH * scale));
    strokeRect(rgba, width, height, boxX, boxY, boxW, boxH, 2, [0, 0, 0, 255]);
    fillRect(rgba, width, height, penX, baselineY, advancePx, 2, [0, 0, 0, 255]);
    penX += advancePx + DEMO_PADDING;
  }

  return { width, height, rgba, summaries };
}

async function renderTextDemoAsync() {
  const bytes = injectedFontBytes();
  if (!(bytes instanceof ArrayBuffer) || bytes.byteLength === 0) {
    fail("injected font bytes are empty");
  }
  if (!opentype || typeof opentype.parse !== "function") {
    fail("opentype.parse is not available");
  }
  const font = opentype.parse(bytes);
  const image = buildMetricImage(font, DEMO_TEXT);
  return {
    width: image.width,
    height: image.height,
    rgba: image.rgba,
    text: DEMO_TEXT,
    fontBytes: bytes.byteLength,
    unitsPerEm: font.unitsPerEm ?? null,
    glyphCount: font.glyphs?.length ?? null,
    ascender: font.ascender ?? null,
    descender: font.descender ?? null,
    advances: image.summaries.map((glyph) => glyph.advance),
    glyphIndices: image.summaries.map((glyph) => glyph.glyphIndex),
  };
}

export { renderTextDemoAsync };
